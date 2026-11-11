//! A Reach weapon's aim assist as literal Weapon Controller inputs. The weapon stores its autoaim
//! and magnetism cones and ranges at `weap +0x2D4..0x2F4`. Shadowkeep's controller definition
//! (`80803A10`) holds `m_aim_assist`, class `80806A9D`, at `+8C0`, and the definition's input
//! initializers name its twelve numeric members as controller inputs 11 to 22 in order. Reader
//! `CB6150` scales inputs 11 and 22 by the autoaim angle scale over a camera factor, inputs 12 to
//! 14 by the autoaim distance scale times that factor, inputs 15 and 17 by the magnetism angle
//! scale over it, 16 and 18 as fractions of 15 and 17, and 19 to 21 by the magnetism distance
//! scale times it. Angles are radians in both games, and source ranges convert at 3.048 metres a
//! world unit. Reach magnetism, the reticle's slowdown and pull over a target, takes both
//! magnetism-scaled angles, friction (15) and the unnamed 17, which every stock table keeps equal.
//! Each value is written only where the donor's stat translator copies a value into that input.
use super::literal;
use crate::halo_reach::rig::METRES_PER_UNIT;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Weapon flag 2, Aim Assists Only When Zoomed, and secondary flag 0, Magnetizes Only When Zoomed.
const ONLY_ZOOMED: u64 = 1 << 2;
const MAGNETIZES_ZOOMED: u64 = 1;

/// The widest angle and longest range the build accepts, in radians and metres.
pub(super) const ANGLE: f32 = std::f32::consts::FRAC_PI_4;
pub(super) const RANGE: f32 = 1000.;

/// Source field, native input and whether the value is a range in world units.
const FIELDS: [(&str, u32, bool); 10] = [
    ("autoaim_angle_radians", 11, false),
    ("autoaim_range", 12, true),
    ("autoaim_falloff_range", 13, true),
    ("autoaim_near_falloff_range", 14, true),
    ("magnetism_angle_radians", 15, false),
    ("magnetism_angle_radians", 17, false),
    ("magnetism_range", 19, true),
    ("magnetism_falloff_range", 20, true),
    ("magnetism_near_falloff_range", 21, true),
    ("deviation_angle_radians", 22, false),
];

/// The aim assist receipt and the literal inputs it writes. `routes` is the donor's routed inputs,
/// or why they could not be read.
pub(super) fn translate(
    gameplay: &Value,
    routes: std::result::Result<&BTreeSet<(u32, u32)>, &str>,
) -> Result<(Value, Vec<Value>)> {
    let source = &gameplay["aim_assist"];
    if !source.is_object() {
        return Ok((
            json!({"status":"native","reason":"The source weapon's aim assist was not read"}),
            Vec::new(),
        ));
    }
    let routes = match routes {
        Ok(routes) => routes,
        Err(reason) => {
            return Ok((
                json!({"status":"native","source":source,"reason":reason}),
                Vec::new(),
            ));
        }
    };
    let mut inputs = Vec::new();
    let mut omitted = Vec::new();
    for (field, input, range) in FIELDS {
        let raw = source[field].as_f64().unwrap_or(f64::NAN) as f32;
        let value = if range { raw * METRES_PER_UNIT } else { raw };
        let limit = if range { RANGE } else { ANGLE };
        if !(0. ..=limit).contains(&value) {
            omitted.push(format!(
                "The source {field} lies outside the native limits of Weapon Controller input {input}"
            ));
        } else if !routes.contains(&(0, input)) {
            omitted.push(format!(
                "The donor's stat translator does not write Weapon Controller input {input}, so the source {field} is not carried"
            ));
        } else {
            inputs.push(literal(0, input, value));
        }
    }
    let flags = gameplay["flags"].as_u64().unwrap_or_default();
    let secondary = gameplay["secondary_flags"].as_u64().unwrap_or_default();
    if flags & ONLY_ZOOMED != 0 {
        omitted.push(
            "The source assists only while zoomed. No native input gates aim assist on zoom, so it also assists unzoomed".to_owned(),
        );
    }
    if secondary & MAGNETIZES_ZOOMED != 0 {
        omitted.push(
            "The source magnetizes only while zoomed. No native input gates magnetism on zoom, so it also magnetizes unzoomed".to_owned(),
        );
    }
    let status = match (inputs.is_empty(), omitted.is_empty()) {
        (true, _) => "native",
        (false, true) => "translated",
        (false, false) => "partial",
    };
    let receipt = json!({"status":status,"source":source,"inputs":inputs,"omitted":omitted,
        "units":"Angles in radians, ranges in metres at 3.048 metres a world unit",
        "limits":["The friction and magnetism falloff fractions, inputs 16 and 18, and the angle and distance scale expressions stay the donor's. A written input holds a literal base, so investment Aim Assistance and Range no longer move it, while component modifiers still do.",
            "Native angles narrow and ranges lengthen with the camera factor reader CB6150 divides and multiplies by, read as the zoom magnification. Whether Reach scales its cones the same way was not read from a source consumer.",
            "That input 17 is the second magnetism angle rests on its scaling and stock values, since its member name is not recovered. Gameplay is unverified."]});
    Ok((receipt, inputs))
}
