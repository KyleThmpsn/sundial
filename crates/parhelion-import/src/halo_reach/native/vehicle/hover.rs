//! Source hover speeds and acceleration on the carrier's retained hover motion.
use super::{curve::Program, motion::number};
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const INSTANCE: u32 = 0x80803cde;
const DEFINITION: u32 = 0x80803171;
// EB7D60 evaluates these two definition expressions as the forward and reverse
// speed targets and hands both limits to the EAD8E0 acceleration clip. The
// active limit applies while driving. Braking is the inferred inactive role.
const FORWARD: usize = 0x210;
const REVERSE: usize = 0x270;
const ACCELERATION_LIMIT: usize = 0x2d0;
const BRAKING_LIMIT: usize = 0x2d4;

/// Read the carrier's constant speed target. The alternate hover owners store
/// `34 00 3E 00` with metadata `[1, 0, 1, 0]` and no provider channels. The
/// ordinary Sparrow's drive-sum program is rejected rather than losing its
/// Engine providers behind a constant.
fn constant(payload: &Payload, owner: u32, at: usize, label: &str) -> Result<f32> {
    ensure!(
        payload.u32(at)? == owner
            && payload.u32(at + 4)? == 0x808089f5
            && payload.u64(at + 48)? == 1
            && payload.u64(at + 56)? == 1
            && payload.u64(at + 64)? == 0,
        "Hover carrier {label} expression differs"
    );
    let code = payload.array_range(at + 16, 1, Some(0x80800009))?;
    let constants = payload.array(at + 32, 16, Some(0x80800090))?;
    ensure!(
        payload.0.get(code).context("Hover expression code")? == [0x34u8, 0, 0x3e, 0]
            && constants.len() == 1,
        "Hover carrier {label} expression is not a constant program"
    );
    payload.f32(constants[0])
}

pub(super) fn build(
    payload: &mut Payload,
    owner: u32,
    source: &Value,
    vehicle: &Value,
    metres: f32,
) -> Result<Value> {
    let instance = payload.pointer(16)?;
    let definition = usize::try_from(payload.u64(instance + 8)?)?;
    ensure!(
        payload.u32(instance - 4)? == INSTANCE && payload.u32(definition - 4)? == DEFINITION,
        "Hover carrier motion classes differ"
    );
    let forward = number(source, "forward_source_units_per_second")? * metres;
    let reverse = number(source, "reverse_source_units_per_second")? * metres;
    let acceleration = number(source, "acceleration_source_units_per_second_squared")? * metres;
    let braking = number(source, "deceleration_source_units_per_second_squared")? * metres;
    ensure!(
        forward > 0. && reverse >= 0. && acceleration > 0. && braking > 0.,
        "Source hover controls are outside the supported domain"
    );
    // Alien scout rows store reverse speed as a positive magnitude, as does
    // the native target, 10 beside forward 13 on the Interceptor.
    let carrier = json!({
        "forward":constant(payload, owner, definition + FORWARD, "forward")?,
        "reverse":constant(payload, owner, definition + REVERSE, "reverse")?,
        "acceleration":payload.f32(definition + ACCELERATION_LIMIT)?,
        "braking":payload.f32(definition + BRAKING_LIMIT)?
    });
    for (member, value) in [(FORWARD, forward), (REVERSE, reverse)] {
        Program::constant(value)?.write(payload, definition + member + 16)?;
    }
    for (member, value) in [(ACCELERATION_LIMIT, acceleration), (BRAKING_LIMIT, braking)] {
        put(&mut payload.0, definition + member, &value.to_le_bytes())?;
    }
    Ok(json!({
        "carrier_owner":format!("{owner:08X}"),"instance_class":INSTANCE,"definition_class":DEFINITION,"definition_offset":definition,
        "source":source,"metres_per_source_unit":metres,
        "forward_metres_per_second":forward,"reverse_metres_per_second":reverse,
        "acceleration_metres_per_second_squared":acceleration,"braking_metres_per_second_squared":braking,
        "carrier":carrier,
        "retained":{
            "boost":{"peak_power":vehicle["boost_peak_power"],"recharge_rate":vehicle["boost_recharge_rate"],"recharge_delay_seconds":vehicle["boost_recharge_delay_seconds"],
                "reason":"The native boost force programs at definition +0x6D0 and +0x810 have no established power, duration or recharge unit."},
            "steering":{"overdamp_cusp_radians":source["overdamp_cusp_radians"],"overdamp_exponent":source["overdamp_exponent"],
                "reason":"No native hover steering lane is established."},
            "lateral":{"left_slide_source_units_per_second":source["left_slide_source_units_per_second"],"right_slide_source_units_per_second":source["right_slide_source_units_per_second"],
                "slide_acceleration_source_units_per_second_squared":source["slide_acceleration_source_units_per_second_squared"],"slide_deceleration_source_units_per_second_squared":source["slide_deceleration_source_units_per_second_squared"],
                "reason":"The native lateral tuple at definition +0x2D8 has no established layout."},
            "drag":{"drag_coefficient":source["drag_coefficient"],"constant_deceleration_source":source["constant_deceleration_source"],"torque_scale":source["torque_scale"],
                "reason":"Native hover drag and force programs stay in use."}
        },
        "gameplay_verified":false,
        "limits":["Source forward and reverse speeds replace the carrier's constant speed targets, and source acceleration and deceleration replace its acceleration limits, at 3.048 metres per source unit. The native unit and the braking role of the inactive limit are inferred. Boost, steering, slides, drag and hover contact stay native."]
    }))
}
