//! A Reach barrel's firing error as literal Barrel inputs. Reach interpolates its error angle pair
//! (`barrel +0x74`) by the barrel's current error, from the lower angle at rest to the upper at
//! full error, and its error falls from full to rest over the deceleration time (`+0x50`). The
//! Shadowkeep Barrel definition (`80803865`) holds the same model: its input initializers name
//! `m_error_deceleration_time` as Barrel input 17, `error_per_recovery` as input 26, and inputs 14
//! and 15 as two unnamed radian angles, the lower at or below the upper in every stock table read.
//! Inputs 14 and 15 are read as the cone at rest and at full error, and take the source pair.
//! Input 17 takes the deceleration time in seconds. Input 26 comes from a native logic row that
//! is not decoded, so the error each shot adds stays the donor's. Each value is written only where
//! the donor's stat translator copies a value into that input.
use super::{literal, number};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The widest cone the build accepts, in radians.
pub(super) const CONE: f32 = 0.5;
/// The narrowest stock cone, 0.01 degrees, written for a source angle of zero. Stock tables never
/// hold a zero cone, so a zero angle's native handling is not established.
const NARROWEST: f32 = 0.000_174_532_9;
/// Barrel flag 14, Error Ignores Zoom.
const IGNORES_ZOOM: u64 = 1 << 14;
/// Distribution function 1, Horizontal Fan, the only one that reads the distribution angle.
const FAN: i64 = 1;

/// The spread receipt and the literal inputs it writes. `routes` is the donor's routed inputs, or
/// why they could not be read.
pub(super) fn translate(
    barrel: &Value,
    routes: std::result::Result<&BTreeSet<(u32, u32)>, &str>,
) -> Result<(Value, Vec<Value>)> {
    let error = [
        number(&barrel["error_angle_radians"][0])?,
        number(&barrel["error_angle_radians"][1])?,
    ];
    let mut receipt = json!({"error_angle_radians":error,
        "error_deceleration_seconds":barrel["error_deceleration_seconds"],
        "minimum_error_radians":barrel["minimum_error_radians"],
        "distribution_function":barrel["distribution_function"],
        "distribution_angle_radians":barrel["distribution_angle_radians"],
        "accuracy_penalties":barrel["accuracy_penalties"],
        "error_penalty_functions":barrel["error_penalty_functions"],
        "projectiles_per_shot":barrel["projectiles_per_shot"]});
    let refuse = |mut receipt: Value, reason: &str| -> Result<(Value, Vec<Value>)> {
        receipt["status"] = json!("native");
        receipt["reason"] = json!(reason);
        Ok((receipt, Vec::new()))
    };
    let routes = match routes {
        Ok(routes) => routes,
        Err(reason) => return refuse(receipt, reason),
    };
    if barrel["projectiles_per_shot"].as_i64().unwrap_or(1) > 1 {
        return refuse(
            receipt,
            "The source fires several projectiles per shot. Whether the native pellet pattern reads the same cone is not established, so the donor's cone stays",
        );
    }
    let mut inputs = Vec::new();
    let mut omitted = Vec::new();
    let ordered = (0. ..=error[1]).contains(&error[0]) && error[1] <= CONE;
    if !ordered {
        omitted.push("The source error angles lie outside the native cone limits".to_owned());
    } else if !(routes.contains(&(2, 14)) && routes.contains(&(2, 15))) {
        omitted.push(
            "The donor's stat translator does not write both Barrel inputs 14 and 15, so its cone stays"
                .to_owned(),
        );
    } else {
        for (input, angle) in [(14, error[0]), (15, error[1])] {
            inputs.push(literal(2, input, angle.max(NARROWEST)));
        }
        if error[0] == 0. {
            omitted.push(
                "The source rests without error, written as the narrowest stock cone of 0.01 degrees"
                    .to_owned(),
            );
        }
    }
    let deceleration = barrel["error_deceleration_seconds"]
        .as_f64()
        .unwrap_or_default() as f32;
    if !(0.01..=30.).contains(&deceleration) {
        omitted.push(
            "The source error deceleration time lies outside native limits, so the donor's recovery stays"
                .to_owned(),
        );
    } else if !routes.contains(&(2, 17)) {
        omitted.push(
            "The donor's stat translator does not write Barrel input 17, so its recovery stays"
                .to_owned(),
        );
    } else {
        inputs.push(literal(2, 17, deceleration));
    }
    omitted.push(
        "The error each shot adds, from the source firing penalty function, is not translated. Native Barrel input 26 comes from an undecoded logic row, so the donor's growth per shot stays"
            .to_owned(),
    );
    if barrel["minimum_error_radians"].as_f64().unwrap_or_default() > 0. {
        omitted.push(
            "The source minimum error, the least angle a shot leaves the aim by, has no native input"
                .to_owned(),
        );
    }
    if barrel["distribution_function"].as_i64() == Some(FAN) {
        omitted.push(
            "The source fans its projectiles across its distribution angle, which no native input carries"
                .to_owned(),
        );
    }
    let flags = barrel["flags"].as_u64().unwrap_or_default();
    let limits = [
        "Reach shrinks its error with zoom magnification unless Error Ignores Zoom is set. The native cone's zoom and hip-fire scaling, Barrel inputs 30 and 31, stay the donor's.",
        "Whether a native cone angle is a half angle, as the source's is, and how Barrel inputs 14 and 15 are sampled are not established, since their reader is not traced. Gameplay is unverified.",
        "Source reload, switch, zoom and jump penalties and the moving, turning and crouched penalty functions are not translated.",
        "A written cone holds a literal base, so investment Range no longer moves it, while component modifiers still do.",
    ];
    // The growth per shot always stays the donor's, so a written cone is partial.
    receipt["status"] = json!(if inputs.is_empty() {
        "native"
    } else {
        "partial"
    });
    receipt["error_ignores_zoom"] = json!(flags & IGNORES_ZOOM != 0);
    receipt["inputs"] = json!(inputs);
    receipt["omitted"] = json!(omitted);
    receipt["limits"] = json!(limits);
    Ok((receipt, inputs))
}
