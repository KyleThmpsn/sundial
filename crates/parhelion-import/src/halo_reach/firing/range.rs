//! The fired projectile's damage over distance. Reach scales a projectile's damage over its air
//! range, and its water range under water, in world units travelled. Shadowkeep has no input
//! that carries it: hitscan falloff reads Weapon Controller distance inputs whose start and end
//! roles are not established, and a projectile damage profile (`80804B02`) has no distance term,
//! its `+1C` being a random extent. The receipt records the source range in metres and why the
//! native falloff applies.
use super::number;
use crate::halo_reach::rig::METRES_PER_UNIT;
use anyhow::Result;
use serde_json::{Value, json};

/// The damage range receipt, when the barrel's projectile scales its damage over a distance.
pub(super) fn receipt(barrel: &Value) -> Result<Option<Value>> {
    let range = &barrel["projectile_damage_range"];
    if !range.is_object() {
        return Ok(None);
    }
    let read = |medium: &str| -> Result<[f32; 2]> {
        Ok([number(&range[medium][0])?, number(&range[medium][1])?])
    };
    let (air, water) = (read("air")?, read("water")?);
    if air[1] <= 0. && water[1] <= 0. {
        return Ok(None);
    }
    Ok(Some(
        json!({"status":"native","air_source_units":air,"water_source_units":water,
        "air_metres":air.map(|units| units * METRES_PER_UNIT),
        "water_metres":water.map(|units| units * METRES_PER_UNIT),
        "reason":"Shadowkeep has no established distance input for this damage. Hitscan falloff reads Weapon Controller distance inputs whose start and end roles are not established, and a projectile damage profile has no distance term",
        "limits":["A donor trace keeps the donor's falloff. An imported carrier deals the same damage at every distance.",
            "How Reach scales damage between the two ends of the range was not read from a source consumer."]}),
    ))
}
