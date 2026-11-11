//! A Reach weapon's firing as literal native stat inputs: the cadence and magazine of one plain
//! trigger and barrel, the heat that locks the barrel until it vents, the battery that ends a
//! plasma weapon, a charging trigger's charge and hold times and the fired projectile's damage
//! range. A charging trigger with a tap barrel and a charged barrel fires its tap barrel as the
//! weapon's shot, and `release` fires the charged barrel's projectile through a private perk once
//! charged. A Spew-Charge trigger fires its charged barrel as the weapon's shot. The weapon's aim
//! assist becomes Weapon Controller inputs 11 to 22 and the barrel's error cone and its recovery
//! Barrel inputs 14, 15 and 17. Each writes its own receipt with a status, a reason and limits.
//! Totals, vents, cones and aim assist are written only where the donor's stat translator copies a
//! value into that input, so the build replaces that literal and nothing else. A charge input
//! also reaches a donor's Charge definition literal where no translator route writes it. `lock`
//! carries the weapon's target tracking onto the donor's lock-on timing.
use crate::tiger::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

mod aim;
mod battery;
mod charge;
pub(super) mod heat;
pub(super) mod lock;
mod range;
pub(super) mod release;
mod spread;

/// The Rounds Per Minute, Magazine and Charge Time display stats. Charge Time shows milliseconds.
const ROUNDS_PER_MINUTE: u32 = 0xff66_4809;
const MAGAZINE: u32 = 0xe6be_4c5a;
const CHARGE_TIME: u32 = 0xb083_53a0;

/// Reach trigger behaviors that charge: 4 charges and 6 spews while it charges. A latch-autofire
/// trigger (2) charges when its secondary action, the one held past its autofire time, is 1.
const CHARGING: [i64; 2] = [4, 6];
const SPEW_CHARGE: i64 = 6;
const LATCH_AUTOFIRE: i64 = 2;
const CHARGE_ACTION: i64 = 1;

/// A single charging trigger that fires one barrel when released before full charge and another
/// once charged, by index into the source triggers and barrels.
#[derive(Clone, Copy, Debug)]
pub(super) struct ChargedPair {
    pub trigger: usize,
    /// The trigger's primary barrel, fired by a release before full charge, or spewed while a
    /// Spew-Charge trigger charges.
    pub tap: usize,
    /// The trigger's secondary barrel, fired once charged.
    pub charged: usize,
    /// Whether the trigger is Spew-Charge (6). It spews its primary barrel only while it charges,
    /// as an aiming tracer, and fires nothing when released early, so its one shot is the
    /// secondary barrel's, fired once charged. That is the shot a native charge weapon fires at
    /// the end of its charge.
    pub spews: bool,
}

impl ChargedPair {
    /// The barrel the weapon fires as its own shot.
    pub fn fired(&self) -> usize {
        if self.spews { self.charged } else { self.tap }
    }

    /// The barrel a private full-charge perk fires in place of the weapon's own shot, when the
    /// weapon's own shot is the tap barrel's.
    pub fn released(&self) -> Option<usize> {
        (!self.spews).then_some(self.charged)
    }

    /// The barrel whose shot is not the weapon's own: the charged barrel of a tap-fired pair and
    /// the spewed barrel of a Spew-Charge pair.
    pub fn other(&self) -> usize {
        if self.spews { self.tap } else { self.charged }
    }

    /// What the firing translation leaves out of the pair.
    fn omission(&self) -> &'static str {
        if self.spews {
            "The primary barrel's spew while the trigger charges is not translated, and the weapon fires the charged barrel's shot"
        } else {
            "The charged barrel's own cadence, heat and battery use are not translated. Its shot draws the tap barrel's magazine"
        }
    }
}

/// The weapon's charged pair: its only live trigger charges for a positive time and names two
/// different barrels, its primary and its secondary, which are the weapon's only barrels. Every
/// other trigger names no barrel. The Plasma Pistol (latch-autofire, secondary action charge),
/// the Spartan Laser and the Target Locator (both spew while charging) have this shape.
pub(super) fn charged_pair(gameplay: &Value) -> Option<ChargedPair> {
    let barrels = gameplay["barrels"].as_array()?;
    let triggers = gameplay["triggers"].as_array()?;
    let index = |value: &Value| value.as_i64().and_then(|index| usize::try_from(index).ok());
    let mut live = triggers
        .iter()
        .enumerate()
        .filter(|(_, trigger)| trigger["primary_barrel"].as_i64().is_some_and(|i| i >= 0));
    let (at, trigger) = live.next()?;
    let unused = triggers.iter().enumerate().all(|(other, row)| {
        other == at
            || [&row["primary_barrel"], &row["secondary_barrel"]]
                .iter()
                .all(|barrel| barrel.as_i64().is_some_and(|i| i < 0))
    });
    let behavior = trigger["behavior"].as_i64()?;
    let charges = CHARGING.contains(&behavior)
        || (behavior == LATCH_AUTOFIRE && trigger["secondary_action"] == CHARGE_ACTION);
    let tap = index(&trigger["primary_barrel"])?;
    let charged = index(&trigger["secondary_barrel"])?;
    (live.next().is_none()
        && unused
        && charges
        && trigger["charging_seconds"].as_f64().is_some_and(|s| s > 0.)
        && barrels.len() == 2
        && tap != charged
        && tap < 2
        && charged < 2)
        .then_some(ChargedPair {
            trigger: at,
            tap,
            charged,
            spews: behavior == SPEW_CHARGE,
        })
}

/// The seconds barrel `barrel` charges before each of its shots, when every trigger naming it fires
/// it as the secondary barrel of a charging trigger once charged and none fires it as a primary
/// barrel. Such a barrel fires at most once per charge, so the shortest charging time among those
/// triggers bounds its cadence.
pub(super) fn charge_wait(gameplay: &Value, barrel: usize) -> Option<f32> {
    let triggers = gameplay["triggers"].as_array()?;
    let names = |value: &Value| {
        value.as_i64().and_then(|index| usize::try_from(index).ok()) == Some(barrel)
    };
    if triggers
        .iter()
        .any(|trigger| names(&trigger["primary_barrel"]))
    {
        return None;
    }
    triggers
        .iter()
        .filter(|trigger| names(&trigger["secondary_barrel"]))
        .map(|trigger| {
            let behavior = trigger["behavior"].as_i64()?;
            let charges = CHARGING.contains(&behavior)
                || (behavior == LATCH_AUTOFIRE && trigger["secondary_action"] == CHARGE_ACTION);
            let seconds = trigger["charging_seconds"].as_f64()? as f32;
            (charges && seconds.is_finite() && seconds > 0.).then_some(seconds)
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .reduce(f32::min)
}

fn number(value: &Value) -> Result<f32> {
    let number = value.as_f64().context("Source firing scalar")? as f32;
    ensure!(number.is_finite(), "Nonfinite source firing scalar");
    Ok(number)
}

fn literal(component: u32, input: u32, value: impl Into<Value>) -> Value {
    json!({"component":component,"input":input,"value":value.into()})
}

/// The firing receipt of a source whose cadence stays the donor's. Aim assist and spread inputs
/// still ride in its inputs, which the build reads.
fn unfired(reason: &str, inputs: &[Value]) -> Value {
    let mut receipt = json!({"status":"native","reason":reason});
    if !inputs.is_empty() {
        receipt["inputs"] = json!(inputs);
    }
    receipt
}

/// The heat or battery receipt `owner` names, when the source has one.
fn owner_receipt<'a>(
    owner: Option<&str>,
    heat: &'a mut Option<(Value, Option<heat::Lockout>)>,
    battery: &'a mut Option<(Value, Option<i64>)>,
) -> Option<&'a mut Value> {
    match owner? {
        "heat" => heat.as_mut().map(|(receipt, _)| receipt),
        _ => battery.as_mut().map(|(receipt, _)| receipt),
    }
}

/// Records an input a receipt wrote, or the reason it could not.
fn note(receipt: Option<&mut Value>, entry: std::result::Result<Value, String>) {
    let Some(receipt) = receipt else { return };
    let (key, value) = match entry {
        Ok(input) => ("inputs", input),
        Err(reason) => ("omitted", Value::from(reason)),
    };
    if !receipt[key].is_array() {
        receipt[key] = json!([]);
    }
    if let Some(rows) = receipt[key].as_array_mut() {
        rows.push(value);
    }
}

/// A qualified receipt is translated when every input it needed was written, else partial.
fn settle(receipt: &mut Value) {
    if receipt["status"].is_null() {
        let missing = receipt["omitted"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty());
        receipt["status"] = json!(if missing { "partial" } else { "translated" });
    }
}

/// The single rate that fires `rounds` in the time a spew barrel takes, climbing linearly from
/// `rates[0]` to `rates[1]` over `acceleration` seconds and then sustaining the upper rate, with the
/// first round at once. A native barrel fires at one rate, so it keeps the source's time to empty
/// one loaded magazine. Returns the rate and that time.
fn magazine_rate(rates: [f32; 2], acceleration: f32, rounds: f32) -> (f32, f32) {
    let intervals = rounds - 1.;
    let climbing = (rates[0] + rates[1]) / 2. * acceleration;
    let seconds = if intervals <= climbing {
        // rates[0] t + (rates[1] - rates[0]) t^2 / (2 acceleration) = intervals
        let a = (rates[1] - rates[0]) / (2. * acceleration);
        (-rates[0] + (rates[0] * rates[0] + 4. * a * intervals).sqrt()) / (2. * a)
    } else {
        acceleration + (intervals - climbing) / rates[1]
    };
    (intervals / seconds, seconds)
}

/// The cadence a plain trigger (behavior 0 spew or 1 latch) fires `barrel` at, and its spin-up
/// receipt. A held spew trigger sustains the upper rate. A latch shot waits out its recovery. A
/// spew barrel that climbs from a lower rate takes the rate that empties `capacity` rounds in the
/// source's time when its acceleration time is accepted, and otherwise fires at its sustained rate
/// at once, which `omitted` records.
fn cadence(
    barrel: &Value,
    behavior: i64,
    capacity: Option<i64>,
    omitted: &mut Vec<&'static str>,
) -> Result<(f32, Value)> {
    let rates = [
        number(&barrel["rounds_per_second"][0])?,
        number(&barrel["rounds_per_second"][1])?,
    ];
    let recovery = number(&barrel["fire_recovery_seconds"])?;
    let mut cadence = rates[1];
    if behavior == 1 && recovery > 0. {
        cadence = cadence.min(1. / recovery);
    }
    let mut spin_up = Value::Null;
    if behavior == 0 && rates[0] < rates[1] {
        let acceleration = barrel["acceleration_seconds"].as_f64().map(|v| v as f32);
        match (acceleration, capacity) {
            (Some(seconds), Some(rounds @ 2..=500)) if seconds > 0. && seconds <= 10. => {
                let (mean, empty) = magazine_rate(rates, seconds, rounds as f32);
                cadence = mean;
                spin_up = json!({"acceleration_seconds":seconds,"magazine":rounds,
                    "seconds_to_empty":empty,"rate":mean});
            }
            _ => omitted.push(
                "The source spin-up from the lower rate is not reproduced, so the weapon fires at its sustained rate at once",
            ),
        }
    }
    Ok((cadence, spin_up))
}

/// A vehicle weapon's firing as literals on its native Barrel, which no stat translator routes.
/// `writable` names the Barrel inputs (component 2) whose definition rows hold a Float32 literal.
/// The primary trigger's primary barrel takes the source cone and, for a plain trigger that fires
/// one shot each time, the source cadence, by the held weapon's rules. A vehicle weapon carries no
/// magazine here, so a spew barrel that climbs from a lower rate fires at its sustained rate.
pub(super) fn vehicle(
    gameplay: &Value,
    writable: &BTreeSet<(u32, u32)>,
) -> Result<(Value, Vec<Value>)> {
    let triggers = gameplay["triggers"]
        .as_array()
        .context("Vehicle weapon triggers")?;
    let barrels = gameplay["barrels"]
        .as_array()
        .context("Vehicle weapon barrels")?;
    let Some(trigger) = triggers.iter().find(|trigger| trigger["input"] == 0) else {
        return Ok((
            json!({"status":"native","reason":"The source weapon has no primary trigger"}),
            Vec::new(),
        ));
    };
    let Some(barrel) = trigger["primary_barrel"]
        .as_u64()
        .and_then(|index| barrels.get(usize::try_from(index).ok()?))
    else {
        return Ok((
            json!({"status":"native","reason":"The source primary trigger names no barrel"}),
            Vec::new(),
        ));
    };
    let (spread, mut inputs) = spread::translate(barrel, Ok(writable))?;
    let behavior = trigger["behavior"]
        .as_i64()
        .context("Source trigger behavior")?;
    let single = [&barrel["shots_per_fire"][0], &barrel["shots_per_fire"][1]]
        .into_iter()
        .all(|shots| matches!(shots.as_i64(), Some(0 | 1)));
    let plain = matches!(behavior, 0 | 1) && number(&trigger["charging_seconds"])? == 0. && single;
    let mut omitted = Vec::new();
    let mut rate = Value::Null;
    let mut spin_up = Value::Null;
    if !plain {
        omitted.push(
            "The source trigger charges, bursts, autofires, tethers or zooms, so the native cadence stays",
        );
    } else {
        let (cadence, climb) = cadence(barrel, behavior, None, &mut omitted)?;
        spin_up = climb;
        if !(0.05..=100.).contains(&cadence) {
            omitted.push("The source cadence lies outside native limits");
        } else if !(0..4).all(|input| writable.contains(&(2, input))) {
            omitted.push("The native Barrel holds no literal for every rate input 0 to 3");
        } else {
            inputs.extend((0..4).map(|input| literal(2, input, cadence)));
            rate = json!(cadence);
        }
    }
    Ok((
        json!({"status":if rate.is_null() { "native" } else { "translated" },
            "trigger_behavior":behavior,"rounds_per_second":barrel["rounds_per_second"],
            "fire_recovery_seconds":barrel["fire_recovery_seconds"],"rate":rate,"spin_up":spin_up,
            "spread":spread,"omitted":omitted,
            "limits":["Native ammunition, heat and reload stay with the carrier's weapon. Gameplay is unverified."]}),
        inputs,
    ))
}

/// The source weapon's firing receipts, keyed as they sit under `source_gameplay`: `firing`,
/// `aim_assist` and `spread` always, `heat`, `battery` and `charge` when the source has them and
/// `damage_range` when its projectile scales damage over distance.
/// `firing.inputs` holds every literal input written and `firing.display_group` the donor's
/// display group with the source rate and magazine. `heat_capacity`, when given, is the magazine a
/// heat lock holds in place of the source's rounds to overheat.
pub(super) fn translate(
    native: &mut Reader,
    donor: u32,
    gameplay: &Value,
    heat_capacity: Option<u32>,
) -> Result<Map<String, Value>> {
    let mut receipts = Map::new();
    // The donor's routed inputs. Aim assist and spread record why when they cannot be read, and
    // the cadence path below requires them.
    let routes = crate::tiger::shadowkeep::translator_routes(native, donor);
    let reason = match &routes {
        Ok(_) => String::new(),
        Err(error) => format!("The donor's stat translator cannot be read: {error:#}"),
    };
    let available = routes.as_ref().map_err(|_| reason.as_str());
    let (aim, mut written) = aim::translate(gameplay, available)?;
    receipts.insert("aim_assist".into(), aim);
    let rows = |name: &str| gameplay[name].as_array().map_or(&[][..], Vec::as_slice);
    // A charged pair fires its tap barrel as the weapon's own shot, and the charged barrel's shot
    // is the charged release receipt's. A Spew-Charge pair fires its charged barrel.
    let pair = charged_pair(gameplay);
    let (barrel, trigger) = match (pair, rows("barrels"), rows("triggers")) {
        (Some(pair), barrels, triggers) => (&barrels[pair.fired()], &triggers[pair.trigger]),
        (None, [barrel], [trigger]) => (barrel, trigger),
        _ => {
            let reason = "The source has several barrels or triggers";
            receipts.insert("spread".into(), json!({"status":"native","reason":reason}));
            receipts.insert("firing".into(), unfired(reason, &written));
            return Ok(receipts);
        }
    };
    let (spread, inputs) = spread::translate(barrel, available)?;
    written.extend(inputs);
    receipts.insert("spread".into(), spread);
    if let Some(range) = range::receipt(barrel)? {
        receipts.insert("damage_range".into(), range);
    }
    let behavior = trigger["behavior"]
        .as_i64()
        .context("Source trigger behavior")?;
    // Reach trigger behavior 0 is spew and 1 is latch. 4 charges, 6 charges and then spews and 7
    // charges a sword lunge. The others autofire, tether or zoom. A barrel that fires no burst
    // stores zero or one shot per fire.
    let single = [&barrel["shots_per_fire"][0], &barrel["shots_per_fire"][1]]
        .into_iter()
        .all(|shots| matches!(shots.as_i64(), Some(0 | 1)));
    let charging = matches!(behavior, 4 | 6 | 7) || pair.is_some();
    let plain = matches!(behavior, 0 | 1) && number(&trigger["charging_seconds"])? == 0. && single;
    if pair.is_none() && (trigger["primary_barrel"] != 0 || !(plain || charging)) {
        receipts.insert(
            "firing".into(),
            unfired(
                "The source trigger bursts, autofires, tethers, zooms or fires another barrel",
                &written,
            ),
        );
        return Ok(receipts);
    }
    let routes = routes?;
    let mut inputs = written;
    let mut omitted = Vec::new();
    let mut shown = Vec::new();

    let Ammunition {
        heat,
        battery,
        capacity,
    } = ammunition(
        gameplay,
        barrel,
        heat_capacity,
        &routes,
        (&mut inputs, &mut shown, &mut omitted),
    )?;
    let rates = [
        number(&barrel["rounds_per_second"][0])?,
        number(&barrel["rounds_per_second"][1])?,
    ];
    let recovery = number(&barrel["fire_recovery_seconds"])?;
    let mut rate = Value::Null;
    let mut spin_up = Value::Null;
    if plain {
        let (cadence, climb) = cadence(barrel, behavior, capacity, &mut omitted)?;
        spin_up = climb;
        if (0.05..=100.).contains(&cadence) {
            inputs.extend((0..4).map(|index| literal(2, index, cadence)));
            shown.push((ROUNDS_PER_MINUTE, (cadence * 60.).round() as i32));
        } else {
            omitted.push("The source cadence lies outside native limits");
        }
        rate = json!(cadence);
    } else {
        let (receipt, charge, milliseconds) = charge::receipt(native, donor, trigger, &routes)?;
        // The display follows the Charge Time written.
        shown.extend(milliseconds.map(|value| (CHARGE_TIME, value)));
        inputs.extend(charge);
        receipts.insert("charge".into(), receipt);
    }

    if let Some(pair) = pair {
        omitted.push(pair.omission());
    }
    let display = firing_display(native, donor, shown)?;
    let mut firing = json!({"status":if plain { "translated" } else { "partial" },
        "trigger_behavior":behavior,"rounds_per_second":rates,"fire_recovery_seconds":recovery,
        "rate":rate,"spin_up":spin_up,"capacity":capacity,"inputs":inputs,
        "display_group":display,"omitted":omitted,
        "limits":["Native trigger mode, investment modifiers and reserves, unless heat or a battery sets them, stay with the donor. Gameplay is unverified."]});
    if charging {
        firing["reason"] = json!(
            "The source trigger charges, so the donor's cadence stays. The charge receipt holds its charge and hold times"
        );
    }
    if let Some(pair) = pair {
        firing["charged_pair"] = json!({"trigger":pair.trigger,"tap_barrel":pair.tap,
            "charged_barrel":pair.charged,"fired_barrel":pair.fired(),"spew_charge":pair.spews,
            "charged_release":"The charged_release receipt holds the charged barrel's shot"});
    }
    receipts.insert("firing".into(), firing);
    if let Some((receipt, _)) = heat {
        receipts.insert("heat".into(), receipt);
    }
    if let Some((receipt, _)) = battery {
        receipts.insert("battery".into(), receipt);
    }
    Ok(receipts)
}

type HeatReceipt = Option<(Value, Option<heat::Lockout>)>;
type BatteryReceipt = Option<(Value, Option<i64>)>;
type FiringOutput<'a> = (
    &'a mut Vec<Value>,
    &'a mut Vec<(u32, i32)>,
    &'a mut Vec<&'static str>,
);

struct Ammunition {
    heat: HeatReceipt,
    battery: BatteryReceipt,
    capacity: Option<i64>,
}

fn ammunition(
    gameplay: &Value,
    barrel: &Value,
    heat_capacity: Option<u32>,
    routes: &std::collections::BTreeSet<(u32, u32)>,
    (inputs, shown, omitted): FiringOutput<'_>,
) -> Result<Ammunition> {
    // Ammunition. A barrel that reloads keeps its magazine. Heat locks a barrel that does not,
    // so the rounds fired from cold before it overheats become the magazine and its vent the
    // reload. A battery that runs out is the whole supply.
    let magazine = barrel["magazine"]
        .as_i64()
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| gameplay["magazines"].get(index));
    let per_round = barrel["rounds_per_shot"] == 1;
    let loaded = magazine
        .and_then(|m| m["loaded"].as_i64())
        .filter(|_| per_round);
    let reloads = loaded.is_some_and(|rounds| rounds > 0)
        && magazine.is_some_and(|m| m["reloaded"].as_i64().is_some_and(|rounds| rounds > 0));
    let mut heat = heat::read(gameplay, barrel)?;
    let mut battery = battery::read(gameplay, barrel)?;
    let Supply {
        lockout,
        capacity,
        total,
    } = supply(
        gameplay,
        magazine,
        per_round,
        (loaded, reloads),
        (&mut heat, &battery),
        heat_capacity,
    )?;
    match capacity {
        Some((rounds @ 1..=500, owner)) => {
            for index in [0, 3] {
                let value = literal(1, index, rounds);
                inputs.push(value.clone());
                note(owner_receipt(owner, &mut heat, &mut battery), Ok(value));
            }
            shown.push((MAGAZINE, rounds as i32));
        }
        Some((_, owner)) => {
            omitted.push("The source barrel draws no single-round magazine within native limits");
            note(
                owner_receipt(owner, &mut heat, &mut battery),
                Err("The source shots lie outside a native magazine's limits".into()),
            );
        }
        None => {
            omitted.push("The source barrel draws no single-round magazine within native limits")
        }
    }
    let capacity = capacity
        .map(|(rounds, _)| rounds)
        .filter(|rounds| (1..=500).contains(rounds));
    if let Some(found) = lockout {
        for index in [1, 2] {
            let entry = if routes.contains(&(3, index)) {
                let value = literal(3, index, found.vent);
                inputs.push(value.clone());
                Ok(value)
            } else {
                Err(format!(
                    "The donor's stat translator does not write Reload input {index}"
                ))
            };
            note(owner_receipt(Some("heat"), &mut heat, &mut battery), entry);
        }
    }
    if let Some((rounds, owner)) = total {
        let entry = if !capacity.is_some_and(|c| (c..=9999).contains(&rounds)) {
            Err("The source rounds carried lie outside native limits".to_owned())
        } else if !routes.contains(&(1, 1)) {
            Err(
                "The donor's stat translator does not write Magazine input 1, the total rounds carried"
                    .to_owned(),
            )
        } else {
            let value = literal(1, 1, rounds);
            inputs.push(value.clone());
            Ok(value)
        };
        note(owner_receipt(Some(owner), &mut heat, &mut battery), entry);
    }
    for (receipt, _) in heat.iter_mut() {
        settle(receipt);
    }
    for (receipt, _) in battery.iter_mut() {
        settle(receipt);
    }
    Ok(Ammunition {
        heat,
        battery,
        capacity,
    })
}

struct Supply {
    lockout: Option<heat::Lockout>,
    capacity: Option<(i64, Option<&'static str>)>,
    total: Option<(i64, &'static str)>,
}

fn supply(
    gameplay: &Value,
    magazine: Option<&Value>,
    per_round: bool,
    (loaded, reloads): (Option<i64>, bool),
    (heat, battery): (&mut HeatReceipt, &BatteryReceipt),
    heat_capacity: Option<u32>,
) -> Result<Supply> {
    let mut lockout = None;
    if let Some((receipt, Some(found))) = heat {
        if reloads {
            receipt["status"] = json!("native");
            receipt["reason"] =
                json!("The source also reloads a magazine, which the native magazine carries");
        } else {
            // An authored capacity rescales the lock: the magazine holds it, the vent still takes
            // the source's time, and the idle refill keeps the source's heat loss at that scale.
            let rounds = heat_capacity.map_or(found.shots, i64::from);
            receipt["magazine"] = json!(rounds);
            if heat_capacity.is_some() {
                receipt["authored_capacity"] = json!(rounds);
            }
            receipt["cooling"] = heat::cooling(&gameplay["heat"], found, rounds)?;
            lockout = Some(heat::Lockout {
                shots: rounds,
                ..*found
            });
        }
    }
    let shots = battery.as_ref().and_then(|(_, shots)| *shots);
    // The magazine and the total rounds carried, each with the receipt that supplies it.
    let mut capacity = loaded.map(|rounds| (rounds, None));
    let mut total = None;
    if let Some(found) = lockout {
        capacity = Some((found.shots, Some("heat")));
        total = match shots {
            Some(shots) => Some((shots, "battery")),
            None => magazine
                .and_then(|m| m["total"].as_i64())
                .filter(|&rounds| per_round && rounds > 0)
                .map(|rounds| (rounds, "heat")),
        };
    } else if let Some(shots) = shots {
        total = Some((shots, "battery"));
        if !reloads {
            capacity = Some((shots, Some("battery")));
        }
    }
    Ok(Supply {
        lockout,
        capacity,
        total,
    })
}

fn firing_display(native: &mut Reader, donor: u32, shown: Vec<(u32, i32)>) -> Result<Value> {
    let mut display = Value::Null;
    if !shown.is_empty() {
        display = crate::stat_group::native(native, donor)?;
    }
    if display.is_object() {
        let definitions = crate::tiger::shadowkeep::stat_definitions(native)?;
        for (stat, value) in shown {
            crate::stat_group::constant(&mut display, &definitions, stat, value)?;
        }
    }
    Ok(display)
}
