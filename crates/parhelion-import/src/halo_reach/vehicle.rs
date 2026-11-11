//! MCC U13 vehicle controls. Source units remain explicit until native translation.
use super::cache::Cache;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
mod collision;
mod shot;

pub(super) fn body(cache: &Cache, model: &super::cache::Tag) -> Result<Value> {
    let Some(tag) = cache.reference(model.address()? + 0x30, Some("phmo"))? else {
        return Ok(Value::Null);
    };
    let at = tag.address()?;
    cache.meta(at, 0x19c)?;
    let regions = cache
        .block(at + 0x130, 0x10)?
        .into_iter()
        .map(|r| {
            let permutations = cache
                .block(r + 4, 0x10)?
                .into_iter()
                .map(|p| {
                    let bodies = cache
                        .block(p + 4, 2)?
                        .into_iter()
                        .map(|b| cache.i16(b))
                        .collect::<Result<Vec<_>>>()?;
                    Ok(json!({"name":cache.string_id(cache.u32(p)?)?,"bodies":bodies}))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"name":cache.string_id(cache.u32(r)?)?,"permutations":permutations}))
        })
        .collect::<Result<Vec<_>>>()?;
    let nodes = cache
        .block(at + 0x13c, 12)?
        .into_iter()
        .map(|r| cache.string_id(cache.u32(r)?))
        .collect::<Result<Vec<_>>>()?;
    let bodies = cache
        .block(at + 0x5c, 0xd0)?
        .into_iter()
        .map(|r| {
            Ok(json!({"node":cache.i16(r)?,"region":cache.i16(r+2)?,
            "permutation":cache.i16(r+4)?,"flags":cache.u16(r+0x18)?,
            "motion":cache.u8(r+0x1c)?,"inertia_scale":cache.floats::<4>(r+0x20)?,
            "damping":cache.floats::<2>(r+0x30)?,"center_offset_source":cache.floats::<3>(r+0x38)?,
            "center_source":cache.floats::<4>(r+0x60)?,"inertia_source":cache.floats::<12>(r+0x70)?,
            "shape":[cache.i16(r+0xa8)?,cache.i16(r+0xaa)?],"mass":cache.f32(r+0xb0)?,
            "mass_override":cache.f32(r+0xbc)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"tag":tag,"mass_scale":cache.f32(at+4)?,"all_permutations_mass":cache.f32(at+8)?,
        "nodes":nodes,"regions":regions,"bodies":bodies,"collision":collision::read(cache,at)?,
        "shot_collision":shot::read(cache,model)?}),
    )
}

pub(super) fn function(cache: &Cache, field: usize) -> Result<Value> {
    cache.meta(field, 20)?;
    let size = usize::try_from(cache.i32(field)?)?;
    ensure!(size <= 1_048_576, "Source vehicle function is too large");
    let bytes = if size == 0 {
        &[][..]
    } else {
        let pointer = cache.u32(field + 12)?;
        ensure!(pointer != 0, "Source vehicle function has no data");
        cache.meta(cache.expand(pointer)?, size)?
    };
    Ok(json!({"bytes":hex::encode(bytes)}))
}

fn steering(cache: &Cache, row: usize) -> Result<Value> {
    Ok(json!({
        "overdamp_cusp_radians":cache.f32(row)?,
        "overdamp_exponent":cache.f32(row+4)?,
        "left_limit_radians":cache.f32(row+8)?,
        "right_limit_radians":cache.f32(row+12)?,
        "rate_radians_per_second":cache.f32(row+16)?
    }))
}

fn jeep(cache: &Cache, row: usize) -> Result<Value> {
    let gears = cache
        .block(row + 0x1c, 0x48)?
        .into_iter()
        .map(|gear| {
            Ok(json!({
                "loaded_torque":function(cache,gear)?,
                "cruising_torque":function(cache,gear+0x18)?,
                "upshift_seconds":cache.f32(gear+0x30)?,
                "upshift_scale":cache.f32(gear+0x34)?,
                "ratio":cache.f32(gear+0x38)?,
                "downshift_seconds":cache.f32(gear+0x3c)?,
                "downshift_scale":cache.f32(gear+0x40)?
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "steering":steering(cache,row)?,
        "engine_moment_source":cache.f32(row+0x14)?,
        "engine_maximum_angular_velocity_source":cache.f32(row+0x18)?,
        "gears":gears,
        "wheel_circumference_source":cache.f32(row+0x50)?,
        "gravity_adjust":cache.f32(row+0x54)?
    }))
}

fn fighter(cache: &Cache, row: usize) -> Result<Value> {
    let mut result = json!({"steering":steering(cache,row)?});
    for (name, offset) in [
        ("forward_source_units_per_second", 0x14),
        ("reverse_source_units_per_second", 0x18),
        ("acceleration_source_units_per_second_squared", 0x1c),
        ("deceleration_source_units_per_second_squared", 0x20),
        ("left_slide_source_units_per_second", 0x24),
        ("right_slide_source_units_per_second", 0x28),
        ("slide_acceleration_source_units_per_second_squared", 0x2c),
        ("slide_deceleration_source_units_per_second_squared", 0x30),
        ("flying_torque_scale", 0x34),
        ("fixed_gun_yaw_radians", 0x38),
        ("fixed_gun_pitch_radians", 0x3c),
        ("maximum_trick_frequency", 0x40),
        ("loop_seconds", 0x44),
        ("roll_seconds", 0x48),
        ("zero_gravity_speed_source", 0x4c),
        ("full_gravity_speed_source", 0x50),
        ("strafe_boost_scale", 0x54),
        ("off_stick_deceleration_scale", 0x58),
        ("cruising_throttle", 0x5c),
        ("dive_speed_scale", 0x60),
    ] {
        result[name] = json!(cache.f32(row + offset)?);
    }
    Ok(result)
}

/// The alien scout row keeps only the overdamp cusp and exponent of the steering
/// record. Its engine, contrail and steering animation fields are presentation.
fn scout(cache: &Cache, row: usize) -> Result<Value> {
    let specific = cache.u8(row + 0x2c)?;
    let specific_name = ["none", "ghost", "spectre", "wraith", "hover_craft"]
        .get(usize::from(specific))
        .copied();
    let mut result = json!({
        "overdamp_cusp_radians":cache.f32(row)?,
        "overdamp_exponent":cache.f32(row+4)?,
        "specific_type":specific,
        "specific_type_name":specific_name,
        "flags":cache.u8(row+0x2d)?
    });
    for (name, offset) in [
        ("forward_source_units_per_second", 0x8),
        ("reverse_source_units_per_second", 0xc),
        ("acceleration_source_units_per_second_squared", 0x10),
        ("deceleration_source_units_per_second_squared", 0x14),
        ("left_slide_source_units_per_second", 0x18),
        ("right_slide_source_units_per_second", 0x1c),
        ("slide_acceleration_source_units_per_second_squared", 0x20),
        ("slide_deceleration_source_units_per_second_squared", 0x24),
        ("slide_speed_at_top_speed_source", 0x28),
        ("drag_coefficient", 0x30),
        ("constant_deceleration_source", 0x34),
        ("torque_scale", 0x38),
    ] {
        result[name] = json!(cache.f32(row + offset)?);
    }
    Ok(result)
}

/// The human plane row of the Pelican and Phantom. MCC U13 513134 scales the forward speed by the
/// vehicle's boost power, chooses the rise speed by the sign of its vertical input, scales every
/// deceleration by the air friction field, turns at a fixed 2.094395 radians per second scaled
/// by the boosting turn scale only while boosting, and banks against sideways motion to a quarter
/// of the maximum roll at full slide speed. Speeds are magnitudes in both directions.
fn plane(cache: &Cache, row: usize) -> Result<Value> {
    let mut result = json!({});
    for (name, offset) in [
        ("forward_source_units_per_second", 0x0),
        ("reverse_magnitude_source_units_per_second", 0x4),
        ("acceleration_source_units_per_second_squared", 0x8),
        ("deceleration_source_units_per_second_squared", 0xc),
        ("left_slide_source_units_per_second", 0x10),
        ("right_slide_source_units_per_second", 0x14),
        ("slide_acceleration_source_units_per_second_squared", 0x18),
        ("slide_deceleration_source_units_per_second_squared", 0x1c),
        ("up_rise_source_units_per_second", 0x20),
        ("down_rise_source_units_per_second", 0x24),
        ("rise_acceleration_source_units_per_second_squared", 0x28),
        ("rise_deceleration_source_units_per_second_squared", 0x2c),
        ("flying_torque_scale", 0x30),
        ("air_friction_deceleration_scale", 0x34),
        ("unknown_38", 0x38),
        ("boosting_turn_rate_scale", 0x3c),
        ("maximum_roll_radians", 0x40),
        ("unknown_44", 0x44),
        ("unknown_48", 0x48),
    ] {
        result[name] = json!(cache.f32(row + offset)?);
    }
    Ok(result)
}

/// The space fighter row of the Sabre and Seraph. MCC U13 51DE3C runs the alien fighter update
/// (516208) step for step with these fields four bytes later from +0x18 on. Its reverse speed is
/// a magnitude. The bank target is sideways motion times a gain within the maximum bank at +0x8C.
/// 1D9C4 follows it as a spring with the stiffness and damping at +0x98 and +0x9C, its rate and
/// acceleration held within +0x94 and +0x90.
fn space(cache: &Cache, row: usize) -> Result<Value> {
    let mut result = json!({"steering":steering(cache,row)?});
    for (name, offset) in [
        ("forward_source_units_per_second", 0x14),
        ("unknown_18", 0x18),
        ("reverse_magnitude_source_units_per_second", 0x1c),
        ("acceleration_source_units_per_second_squared", 0x20),
        ("deceleration_source_units_per_second_squared", 0x24),
        ("left_slide_source_units_per_second", 0x28),
        ("right_slide_source_units_per_second", 0x2c),
        ("slide_acceleration_source_units_per_second_squared", 0x30),
        ("slide_deceleration_source_units_per_second_squared", 0x34),
        ("flying_torque_scale", 0x38),
        ("strafe_boost_scale", 0x50),
        ("off_stick_deceleration_scale", 0x54),
        ("dive_speed_scale", 0x58),
        ("bank_gain", 0x88),
        ("maximum_bank_radians", 0x8c),
        ("bank_acceleration_limit_radians_per_second_squared", 0x90),
        ("bank_rate_limit_radians_per_second", 0x94),
        ("bank_stiffness_per_second_squared", 0x98),
        ("bank_damping_per_second", 0x9c),
    ] {
        result[name] = json!(cache.f32(row + offset)?);
    }
    Ok(result)
}

/// The vtol block of the Falcon (0xD0 bytes), named by the tag definitions and read by MCC U13
/// 5192F4 and its helpers. The vertical input passes through trigger to throttle and becomes lift
/// from the maximum up or down acceleration. Released, the retained lift bleeds over the vertical
/// deceleration time and is held at most at gravity. With altitude lock set, the altitude lock
/// offset and velocity coefficients hold the locked altitude. Descent to boost raises upward
/// authority while descending, over the maximum downward speed. 51A080 blends the speed
/// interpolated rows by a transition state. Forward speed over the interpolation speed domain caps
/// the forward throttle through the speed throttle ceiling, the interpolation acceleration
/// integrates the state, and the interpolation blend maps it to the row weight, so the last row
/// is forward flight. Each row's maximum left and forward accelerations tilt the lift on both
/// lift markers (51AC90), and its magic angular acceleration torques the vehicle back toward up.
/// The prop rotation speed bounds are cycles per second from idle to full throttle. 51A804 lands
/// and takes off with the landing fields.
fn vtol(cache: &Cache, row: usize) -> Result<Value> {
    let speed_interpolated = cache
        .block(row + 0x9c, 0x20)?
        .into_iter()
        .map(|set| {
            let mut result = json!({});
            for (name, offset) in [
                ("rotor_damping", 0x0),
                (
                    "maximum_left_acceleration_source_units_per_second_squared",
                    0x4,
                ),
                (
                    "maximum_forward_acceleration_source_units_per_second_squared",
                    0x8,
                ),
                ("drag_coefficient", 0xc),
                ("constant_deceleration", 0x10),
                ("upright_acceleration_exponent", 0x14),
                ("upright_acceleration_scale", 0x18),
                ("upright_acceleration_k", 0x1c),
            ] {
                result[name] = json!(cache.f32(set + offset)?);
            }
            Ok(result)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut result = json!({
        "turning":{"left_limit_radians":cache.f32(row)?,"right_limit_radians":cache.f32(row+4)?,
            "rate_radians_per_second":cache.f32(row+8)?},
        "left_lift_marker":cache.string_id(cache.u32(row+0xc)?)?,
        "right_lift_marker":cache.string_id(cache.u32(row+0x10)?)?,
        "thrust_marker":cache.string_id(cache.u32(row+0x14)?)?,
        "trigger_to_throttle":function(cache,row+0x18)?,
        "descent_to_boost":function(cache,row+0x2c)?,
        "speed_throttle_ceiling":function(cache,row+0x60)?,
        "interpolation_acceleration":function(cache,row+0x74)?,
        "interpolation_blend":function(cache,row+0x88)?,
        "speed_interpolated":speed_interpolated,
        "prop_rotation_cycles_per_second":cache.floats::<2>(row+0xb8)?,
    });
    for (name, offset) in [
        ("maximum_downward_source_units_per_second", 0x40),
        (
            "maximum_up_acceleration_source_units_per_second_squared",
            0x44,
        ),
        (
            "maximum_down_acceleration_source_units_per_second_squared",
            0x48,
        ),
        ("vertical_deceleration_seconds", 0x4c),
        ("lift_arm_pivot_source_units", 0x50),
        ("maximum_turn_acceleration", 0x54),
        ("turn_acceleration_gain", 0x58),
        ("interpolation_speed_domain_source_units_per_second", 0x5c),
        ("lift_angle_acceleration_radians_per_second_squared", 0xa8),
        (
            "render_lift_angle_acceleration_radians_per_second_squared",
            0xac,
        ),
        ("altitude_lock_offset_coefficient", 0xb0),
        ("altitude_lock_velocity_coefficient", 0xb4),
        ("landing_seconds", 0xc0),
        ("takeoff_seconds", 0xc4),
        ("landing_source_units_per_second", 0xc8),
        ("landing_angular_radians_per_second", 0xcc),
    ] {
        result[name] = json!(cache.f32(row + offset)?);
    }
    Ok(result)
}

pub(super) fn controls(cache: &Cache, kind: &str, row: usize) -> Result<Value> {
    match kind {
        "vtol" => vtol(cache, row),
        "human_jeep" => jeep(cache, row),
        "alien_fighter" => fighter(cache, row),
        "alien_scout" => scout(cache, row),
        "human_plane" => plane(cache, row),
        "space_fighter" => space(cache, row),
        _ => Ok(json!({})),
    }
}

pub(super) fn tricks(cache: &Cache, object: usize) -> Result<Vec<Value>> {
    cache
        .block(object + 0x5d8, 0x20)?
        .into_iter()
        .map(|row| {
            Ok(json!({
                "animation":cache.string_id(cache.u32(row)?)?,
                "activation":cache.u8(row+4)?,
                "velocity_preservation":cache.u8(row+5)?,
                "flags":cache.u8(row+6)?,
                "camera_interpolation_seconds":cache.f32(row+8)?,
                "exit_seconds":cache.f32(row+12)?,
                "exit_camera_interpolation_seconds":cache.floats::<2>(row+16)?,
                "exit_displacement_source":cache.f32(row+24)?,
                "cooldown_seconds":cache.f32(row+28)?
            }))
        })
        .collect()
}

pub(super) fn contacts(cache: &Cache, object: usize) -> Result<Value> {
    let wheels = cache
        .block(object + 0x5c8, 0x34)?
        .into_iter()
        .map(|row| {
            Ok(json!({
                "marker":cache.string_id(cache.u32(row)?)?,
                "flags":cache.u32(row+4)?,
                "mass_source":cache.f32(row+8)?,
                "radius_source":cache.f32(row+12)?,
                "damaged_radius_source":cache.f32(row+16)?,
                "friction_velocity_source":cache.f32(row+20)?,
                "brake_moving_friction":cache.f32(row+24)?,
                "brake_friction":cache.f32(row+28)?,
                "brake_friction_velocity_source":cache.f32(row+32)?,
                "material":cache.string_id(cache.u32(row+36)?)?,
                "material_index":cache.i16(row+40)?,
                "destroyed_state":cache.i16(row+42)?,
                "region":cache.string_id(cache.u32(row+44)?)?,
                "region_index":cache.i32(row+48)?
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut result = json!({"flags":cache.u32(object+0x584)?,"wheels":wheels,
        "suspension_count":cache.i32(object+0x5b8)?,
        "anti_gravity_count":cache.block(object+0x5bc,0x48)?.len()});
    for (index, name) in [
        "ground_friction",
        "ground_depth_source",
        "ground_damping",
        "ground_moving_friction",
        "maximum_slope_cosine_0",
        "maximum_slope_cosine_1",
        "maximum_normal_force_contribution",
        "anti_gravity_bank_lift",
        "steering_bank_reaction_scale",
        "gravity_scale",
    ]
    .into_iter()
    .enumerate()
    {
        result
            .as_object_mut()
            .context("Vehicle contact object")?
            .insert(name.into(), json!(cache.f32(object + 0x588 + index * 4)?));
    }
    Ok(result)
}
