//! A Reach battery as the native total of rounds carried. A plasma barrel ages its weapon a fixed
//! amount each time it fires and, when the weapon cannot fire at full age, stops there, so the
//! battery holds a fixed number of shots. Magazine input 1 holds the total rounds a native weapon
//! carries, its magazine included, since the inspect display shows input 1 less the capacity as
//! reserves.
use super::number;
use anyhow::Result;
use serde_json::{Value, json};

/// Weapon flag 6, Cannot Fire At Maximum Age.
const STOPS_AT_FULL_AGE: u64 = 1 << 6;
/// Barrel flag 11, Can Fire At Maximum Age.
const FIRES_AT_FULL_AGE: u64 = 1 << 11;

fn scalar(value: &Value) -> Result<f32> {
    if value.is_null() {
        return Ok(0.);
    }
    number(value)
}

fn refuse(mut receipt: Value, reason: &str) -> Result<Option<(Value, Option<i64>)>> {
    receipt["status"] = json!("native");
    receipt["reason"] = json!(reason);
    Ok(Some((receipt, None)))
}

/// The battery receipt and shots of a weapon that ages, or `None` for one that does not. A
/// receipt without shots already holds its status and reason.
pub(super) fn read(gameplay: &Value, barrel: &Value) -> Result<Option<(Value, Option<i64>)>> {
    let per_round = scalar(&barrel["age_per_round"])?;
    let age = &gameplay["age"];
    let external = scalar(&age["external_aging"])?;
    if per_round <= 0. && external <= 0. {
        return Ok(None);
    }
    let mut receipt = json!({"age_per_round":per_round,
        "campaign_age_per_round":barrel["campaign_age_per_round"],"source":age});
    if per_round <= 0. {
        let uses = (1. / f64::from(external)).ceil();
        receipt["uses"] = json!(uses);
        return refuse(
            receipt,
            "The source ages with each melee use, which the native melee weapon's own ammunition does not follow",
        );
    }
    let weapon = gameplay["flags"].as_u64().unwrap_or_default();
    let flags = barrel["flags"].as_u64().unwrap_or_default();
    if weapon & STOPS_AT_FULL_AGE == 0 || flags & FIRES_AT_FULL_AGE != 0 {
        return refuse(receipt, "The source keeps firing at full age");
    }
    // A battery that empties in a whole number of shots stores that reciprocal only as nearly as
    // a 32-bit float can, so 0.0025 reads as 400 shots rather than 401.
    let exact = 1. / f64::from(per_round);
    let nearest = exact.round();
    let shots = (if (exact - nearest).abs() < 1e-3 {
        nearest
    } else {
        exact.ceil()
    }) as i64;
    if !(1..=9999).contains(&shots) {
        return refuse(
            receipt,
            "The source battery holds more shots than native ammunition counts",
        );
    }
    let mut omitted = Vec::<Value>::new();
    if scalar(&age["misfire_chance"])? > 0. {
        omitted.push(json!(format!(
            "Misfires from age {} with a chance of {} at full age have no native counterpart",
            scalar(&age["misfire_start"])?,
            scalar(&age["misfire_chance"])?
        )));
    }
    if scalar(&age["rate_of_fire_penalty"])? != 0. || scalar(&age["heat_recovery_penalty"])? != 0. {
        omitted.push(json!(
            "The source age rate of fire and heat recovery penalties are not translated"
        ));
    }
    receipt["shots"] = json!(shots);
    receipt["native"] = json!("Magazine input 1, the total rounds carried");
    receipt["inputs"] = json!([]);
    receipt["omitted"] = json!(omitted);
    receipt["limits"] = json!([
        "Native ammunition pickups refill the reserves. A Reach battery never recharges.",
        "The battery uses the multiplayer age per round. The campaign value is recorded.",
        "Native Magazine inputs 2 and 4, which Inventory Size also sets, keep the donor's values. Gameplay is unverified."
    ]);
    Ok(Some((receipt, Some(shots))))
}
