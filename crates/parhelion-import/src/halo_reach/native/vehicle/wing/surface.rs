//! Plan restoring torque and rotational damping around the selected source body.
use super::super::{body::Body, curve::Program, motion::number};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(super) enum Lift {
    Constant(f32),
    Incidence(f32),
}

impl Lift {
    pub(super) fn program(self, reference: f32) -> Result<Program> {
        match self {
            Self::Constant(value) => {
                let program = Program::constant(value)?;
                if value == 0. {
                    Ok(program)
                } else {
                    program.airspeed(reference, true)
                }
            }
            Self::Incidence(gain) => Program::damping_lift(gain)?.airspeed(reference, false),
        }
    }

    pub(super) fn uses_airspeed(self) -> bool {
        !matches!(self, Self::Constant(value) if value == 0.)
    }

    pub(super) fn report(self) -> Value {
        match self {
            Self::Constant(value) => json!({"constant":value}),
            Self::Incidence(gain) => json!({"sine_incidence_gain":gain}),
        }
    }
}

pub(super) struct Surface {
    pub name: &'static str,
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    pub axis: u8,
    pub area: f32,
    pub lift: Lift,
    pub drag: f32,
    pub response: Program,
}

pub(super) struct Plan {
    pub surfaces: Vec<Surface>,
    pub reference_speed: f32,
    pub report: Value,
}

/// A vtol's upright response, per unit of moment of inertia, from its forward flight speed
/// interpolated row. Reach `5192F4` (MCC U13) turns the body back toward up about the horizontal
/// axis between body up and world up (`519B98` to `519C79`), at an angular acceleration of
/// `k * (scale * tilt)^exponent` for a tilt in radians. From `519DD0` it removes roll and pitch rate
/// at the rotor damping rate per second and leaves yaw rate alone. The stiffness here is that
/// acceleration at the attitude limit divided by the limit, so a full attitude error torques the
/// body as Reach does at that tilt. Both source products also carry factors this route reads as
/// one, an inference: physics state `+0x74`, the flying fraction, a resolved record's `+0x3A0`
/// and a frame value read as the time step.
struct Upright {
    stiffness: f32,
    damping: f32,
    report: Value,
}

fn upright(controls: &Value, limit: f32) -> Result<Option<Upright>> {
    if controls["kind"] != "vtol" {
        return Ok(None);
    }
    let Some(row) = controls["source"]["speed_interpolated"]
        .as_array()
        .and_then(|rows| rows.last())
    else {
        return Ok(None);
    };
    let exponent = number(row, "upright_acceleration_exponent")?;
    let scale = number(row, "upright_acceleration_scale")?;
    let k = number(row, "upright_acceleration_k")?;
    let damping = number(row, "rotor_damping")?;
    // A row without a restoring term or rate damping keeps the authored response.
    if [exponent, scale, k, damping].iter().any(|v| *v <= 0.) {
        return Ok(None);
    }
    let acceleration = k * (scale * limit).powf(exponent);
    let stiffness = acceleration / limit;
    ensure!(
        stiffness.is_finite() && stiffness > 0.,
        "Invalid source vtol upright response"
    );
    let frequency = stiffness.sqrt();
    Ok(Some(Upright {
        stiffness,
        damping,
        report: json!({"exponent":exponent,"scale":scale,"k":k,"rotor_damping_per_second":damping,
            "attitude_limit_radians":limit,"angular_acceleration_at_limit":acceleration,
            "frequency_per_second":frequency,"damping_ratio":damping / (2. * frequency),
            "axes":["roll","pitch"],"source":"Forward flight speed interpolated row"}),
    }))
}

pub(super) fn plan(
    size: [f32; 3],
    controls: &Value,
    body: &Body,
    gravity: f32,
    maneuvers: bool,
    metres: f32,
) -> Result<Plan> {
    let forward = number(controls, "forward_source_units_per_second")? * metres;
    let acceleration = number(controls, "acceleration_source_units_per_second_squared")? * metres;
    let deceleration = number(controls, "deceleration_source_units_per_second_squared")? * metres;
    let (cruise, _) = super::super::flight::thrust(controls)?;
    let bank = number(&controls["steering"], "left_limit_radians")?;
    let rate = number(&controls["steering"], "rate_radians_per_second")?;
    // A source without gravity, such as a dropship, holds altitude with no baseline lift. The
    // contact definition carries the same scale, so its native body is not pulled down either.
    ensure!(
        [forward, acceleration, deceleration, cruise, bank, rate]
            .iter()
            .all(|v| v.is_finite() && *v > 0.)
            && gravity.is_finite()
            && gravity >= 0.,
        "Invalid source flight force controls"
    );
    let area = size[0] * size[1] * 0.25;
    let attitude_area = area * 0.35;
    let dynamic = 0.6125 * forward * forward;
    let lift = body.mass * 12.713999 * gravity / (dynamic * cruise * area * 2.);
    let drag = body.mass * acceleration / (dynamic * area);
    let damping = body.mass * deceleration / (dynamic * area);
    let roll_lever = size[1] * 0.25;
    let turn_lever = size[0] * 0.25;
    let center = body.center;
    let left = [center[0], center[1] + roll_lever, center[2]];
    let right = [center[0], center[1] - roll_lever, center[2]];
    let rear = [center[0] - turn_lever, center[1], center[2]];
    let front = [center[0] + turn_lever, center[1], center[2]];
    let identity = [0., 0., 0., 1.];
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let vertical = [-half, 0., 0., half];
    let frontal = [0., half, 0., half];

    // These are authored response targets, not Reach's native flight solver.
    // Calibrate each axis against the actual translated moment of inertia.
    // Opposed incidence surfaces brake rotation without requiring a turn input.
    let frequency = rate / bank * 0.3;
    let mut frequencies = [frequency * 1.25, frequency, frequency];
    let ratio = 1.05;
    let upright = upright(controls, bank)?;
    if let Some(upright) = &upright {
        frequencies[0] = upright.stiffness.sqrt();
        frequencies[1] = frequencies[0];
    }
    // A vtol's stiff upright torque comes from a front and rear pitch pair, as roll comes from a
    // left and right pair, so it turns the body without a net heave.
    let pitch_pair = upright.is_some();
    let mut authority = [0.; 3];
    let mut stability = [0.; 3];
    for axis in 0..3 {
        let moment = body.inertia[axis][axis] as f32;
        let (stiffness, angular_damping) = match &upright {
            Some(upright) if axis < 2 => (moment * upright.stiffness, moment * upright.damping),
            _ => (
                moment * frequencies[axis] * frequencies[axis],
                2. * ratio * moment * frequencies[axis],
            ),
        };
        let (surface_area, lever, surfaces) = if axis == 0 {
            (area, roll_lever, 2.)
        } else if axis == 1 && pitch_pair {
            (attitude_area, turn_lever, 2.)
        } else {
            (attitude_area, turn_lever, 1.)
        };
        authority[axis] = stiffness * bank / (dynamic * surface_area * lever * surfaces);
        stability[axis] = angular_damping / (2. * 0.6125 * forward * surface_area * lever * lever);
    }
    ensure!(
        authority
            .iter()
            .chain(&stability)
            .all(|v| v.is_finite() && *v > 0.),
        "Invalid flight attitude calibration"
    );
    let mut surfaces = Vec::new();
    for (name, position, right_side) in
        [("flight_left", left, false), ("flight_right", right, true)]
    {
        surfaces.push(Surface {
            name,
            position,
            orientation: identity,
            axis: 1,
            area,
            lift: Lift::Constant(authority[0]),
            drag: 1.,
            response: if maneuvers {
                Program::roll_surface(right_side)?
            } else {
                Program::ramp([-1., 1.], if right_side { [1., -1.] } else { [-1., 1.] })?
            },
        });
    }
    for (name, orientation, axis, gain, response) in [
        ("flight_pitch", identity, 3, authority[1], [1., -1.]),
        ("flight_yaw", vertical, 2, authority[2], [-1., 1.]),
    ] {
        surfaces.push(Surface {
            name,
            position: rear,
            orientation,
            axis,
            area: attitude_area,
            lift: Lift::Constant(gain),
            drag: 1.,
            response: Program::ramp([-bank, bank], response)?,
        });
    }
    // Lift ahead of the mass centre turns the nose the other way, so the front surface's response
    // is reversed.
    if pitch_pair {
        surfaces.push(Surface {
            name: "flight_pitch_front",
            position: front,
            orientation: identity,
            axis: 3,
            area: attitude_area,
            lift: Lift::Constant(authority[1]),
            drag: 1.,
            response: Program::ramp([-bank, bank], [-1., 1.])?,
        });
    }
    for (name, orientation, coefficient) in [
        ("flight_forward_drag", frontal, drag),
        ("flight_slide_drag", vertical, damping),
        ("flight_vertical_drag", identity, damping),
    ] {
        surfaces.push(Surface {
            name,
            position: center,
            orientation,
            axis: 0,
            area,
            lift: Lift::Constant(0.),
            drag: coefficient,
            response: Program::constant(1.)?,
        });
    }
    // Baseline lift at the mass centre cannot roll the body when yaw changes
    // the relative airspeed of its left and right sides. The roll surfaces
    // supply only the commanded difference, with no baseline lift of their own.
    surfaces.push(Surface {
        name: "flight_lift",
        position: center,
        orientation: identity,
        axis: 0,
        area: area * 2.,
        lift: Lift::Constant(lift),
        drag: 0.,
        response: Program::constant(1.)?,
    });
    for (name, position, orientation, area, gain) in [
        (
            "flight_roll_damping_left",
            left,
            identity,
            area,
            stability[0],
        ),
        (
            "flight_roll_damping_right",
            right,
            identity,
            area,
            stability[0],
        ),
        (
            "flight_pitch_damping_front",
            front,
            identity,
            attitude_area,
            stability[1],
        ),
        (
            "flight_pitch_damping_rear",
            rear,
            identity,
            attitude_area,
            stability[1],
        ),
        (
            "flight_yaw_damping_front",
            front,
            vertical,
            attitude_area,
            stability[2],
        ),
        (
            "flight_yaw_damping_rear",
            rear,
            vertical,
            attitude_area,
            stability[2],
        ),
    ] {
        surfaces.push(Surface {
            name,
            position,
            orientation,
            axis: 0,
            area,
            lift: Lift::Incidence(gain),
            drag: 0.,
            response: Program::constant(1.)?,
        });
    }
    ensure!(
        surfaces.len() <= 16,
        "Flight surfaces exceed native work capacity"
    );
    Ok(Plan {
        surfaces,
        reference_speed: forward,
        report: json!({"reference_speed_metres_per_second":forward,
            "low_speed_compensation":{"minimum_speed_metres_per_second":forward * 0.2,"maximum_lift_gain":25.,"maximum_damping_gain":5.,"speed_source":"Primary body linear speed","zero_flow_force":0.},
            "response_frequency_per_second":frequencies,"target_damping_ratio":ratio,
            "upright":upright.map(|upright| upright.report),"pitch_surface_pair":pitch_pair,
            "control_lift_coefficients":authority,"damping_lift_gains":stability,
            "source_frame_inertia_kg_metres_squared":body.inertia,
            "neutral_lift_at_mass_center":true,"gameplay_verified":false}),
    })
}
