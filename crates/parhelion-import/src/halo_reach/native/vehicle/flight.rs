//! An authored flight response on Shadowkeep's registered flight implementation.
use super::{
    curve::Program,
    motion::{Callback, Contract, INSTANCE, Input, Motion, Writer, number},
};
use crate::{
    presentation::{Graph, append, put},
    tiger::{allocation::Row, reader::Reader},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

// Keep both mutable channel bindings in the instance span copied by the loader.
// Their definitions and the numeric programs remain in the resource's schema.
const DEFINITION: usize = INSTANCE + 0x368;

pub(super) fn thrust(source: &Value) -> Result<(f32, f32)> {
    let cruise = number(source, "cruising_throttle")?;
    ensure!(
        (0.0..=1.0).contains(&cruise) && cruise > 0.,
        "Invalid source cruise thrust"
    );
    // With quadratic drag, 25% more thrust raises steady cruise speed by
    // approximately 12%. Preserve the existing boost demand above that cruise.
    Ok(((cruise * 1.25).min(1.), cruise * 2.))
}

fn channel(
    w: &mut Writer,
    slot: usize,
    expression: usize,
    definition: usize,
    method: u32,
) -> Result<Input> {
    let header = INSTANCE + 0x270 + slot * 0x80;
    let input = header + 16;
    ensure!(
        slot < 2 && input + 96 < DEFINITION,
        "Flight input span differs"
    );
    // The physical stream walker needs the array envelope even when the
    // expression already holds a valid descriptor for these mutable inputs.
    put(&mut w.payload.0, header - 4, &0x80809fbdu32.to_le_bytes())?;
    put(&mut w.payload.0, header, &1u64.to_le_bytes())?;
    put(&mut w.payload.0, header + 8, &0x80809788u32.to_le_bytes())?;
    put(&mut w.payload.0, expression + 0x20, &1u64.to_le_bytes())?;
    put(
        &mut w.payload.0,
        expression + 0x28,
        &(header as i64 - (expression + 0x28) as i64).to_le_bytes(),
    )?;
    append(
        &mut w.payload.0,
        definition + 0x40,
        0x80809789,
        &[0; 40],
        40,
    )?;
    let link = w.payload.array(definition + 0x40, 40, Some(0x80809789))?[0];
    w.pair(input, link, (0x80809788, 0x80809789), Some(expression))?;
    for field in [0x20, 0x24, 0x30, 0x38, 0x3c] {
        put(&mut w.payload.0, input + field, &u32::MAX.to_le_bytes())?;
    }
    put(&mut w.payload.0, link + 24, &0x808097c1u64.to_le_bytes())?;
    Ok(Input {
        offset: link,
        method,
    })
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    source: &Value,
    activation_priority: i16,
    maneuvers: bool,
    metres: f32,
) -> Result<Motion> {
    let template = native.tag(0x80bfaf05, Some(0x80809c36))?;
    let mut w = Writer::new(&template, (0x80803cf2, 0x80803cf1), 0x360, 0x420)?;
    let steering = &source["steering"];
    let left = number(steering, "left_limit_radians")?;
    let right = number(steering, "right_limit_radians")?;
    let rate = number(steering, "rate_radians_per_second")?;
    let forward = number(source, "forward_source_units_per_second")? * metres;
    let reverse_speed = number(source, "reverse_source_units_per_second")? * metres;
    let acceleration = number(source, "acceleration_source_units_per_second_squared")? * metres;
    let deceleration = number(source, "deceleration_source_units_per_second_squared")? * metres;
    let (cruise, boost) = thrust(source)?;
    ensure!(
        left > 0.
            && right < 0.
            && (left + right).abs() < 0.0001
            && left < std::f32::consts::FRAC_PI_2
            && rate > 0.
            && forward > 0.
            && reverse_speed <= 0.
            && acceleration > 0.
            && deceleration > 0.
            && cruise > 0.
            && cruise <= 1.,
        "Source flight controls are outside the authored response domain"
    );
    // The authored drag response is quadratic. The squared source speed ratio
    // therefore sets the reverse demand relative to the existing cruise demand.
    let reverse = cruise * (reverse_speed / forward).powi(2);
    let bank = left.to_degrees();
    // EB22C0 supplies heading and bank errors in degrees and converts pitch
    // results to radians. Pitch follows the retained unit's relative aim channel.
    // It therefore corrects toward the mouse aim without treating W/S as pitch.
    for (i, d, program) in [
        (0xa0, 0xc0, Program::ramp([0., bank], [0., bank])?),
        (0xd0, 0x120, Program::ramp([0., bank], [0., 1.])?),
        (0x100, 0x180, Program::constant(0.)?),
        (0x130, 0x1e0, Program::flight_pitch(bank)?),
        (0x160, 0x240, Program::constant(0.)?),
        (0x190, 0x2a0, Program::ramp([0., 1.], [0., bank])?),
        (0x1c0, 0x330, Program::constant(acceleration)?),
    ] {
        w.scalar(INSTANCE + i, DEFINITION + d, INSTANCE, program)?;
    }
    // Native smoothing is an exponential approach. The source turn rate sets
    // the time to traverse the source bank limit, rather than changing its unit.
    let response_gain = 2.;
    let response = rate / left * response_gain;
    for (field, value) in [
        (0x320, 1.),
        (0x324, response),
        (0x328, response),
        (0x32c, response),
    ] {
        put(&mut w.payload.0, DEFINITION + field, &value.to_le_bytes())?;
    }
    let boost_i = INSTANCE + 0x1f0;
    let boost_d = DEFINITION + 0x390;
    w.pair(boost_i, boost_d, (0x80803cf5, 0x80803cf6), Some(INSTANCE))?;
    w.scalar(
        boost_i + 0x20,
        boost_d + 0x10,
        boost_i,
        Program::flight_throttle(cruise, reverse, boost)?,
    )?;
    for (field, value) in [
        (0x70, acceleration / forward),
        (0x74, deceleration / forward),
        (0x78, 0.),
        (0x7c, boost),
        (0x80, 0.),
    ] {
        put(&mut w.payload.0, boost_d + field, &value.to_le_bytes())?;
    }
    // Signed movement requests forward or reverse thrust, with boost taking
    // priority. Releasing both requests zero. Energy stays available. Source fuel, tricks
    // and gravity transitions remain separate contracts.
    put(&mut w.payload.0, boost_i + 0x54, &1f32.to_le_bytes())?;
    let inputs = vec![
        channel(&mut w, 0, INSTANCE + 0x130, DEFINITION + 0x1e0, 55)?,
        channel(&mut w, 1, boost_i + 0x20, boost_d + 0x10, 60)?,
    ];
    w.allocations = vec![
        Row::indirect(0xe43d6460, Row::inputs(1)),
        Row::named(
            0xac0b74ca,
            u32::MAX,
            0,
            vec![Row::indirect(0x91aa6cb1, Row::inputs(1))],
        ),
    ];
    let mut callbacks = Vec::new();
    for (offset, interface, instance_interface, methods, key, priority, symbol) in [
        (
            0x48,
            0x80803cf4,
            0x80803cf3,
            vec![0, 1],
            0x20389238,
            0,
            "vehicle-flight-interface",
        ),
        (
            0x60,
            0x80809ade,
            0x80809add,
            vec![3],
            0x95a60f29,
            activation_priority,
            "vehicle-flight-activate",
        ),
        (
            0x78,
            0x808097bb,
            0x808097ba,
            vec![4],
            0xab076dc7,
            -7,
            "vehicle-flight-update",
        ),
        (
            0x90,
            0x80803d4b,
            0x80803d4a,
            vec![5, 6],
            0x18dba5e9,
            0,
            "vehicle-flight-motion",
        ),
        (
            0xa8,
            0x80809494,
            0x80809493,
            vec![8],
            0xecc9dfcb,
            0,
            "vehicle-flight-input",
        ),
    ] {
        let offset = DEFINITION + offset;
        let metadata = w.provider(
            graph,
            offset,
            DEFINITION,
            Contract {
                interface,
                instance_interface,
                implementation: 0x80803cf2,
                methods: &methods,
                symbol,
                priority,
            },
        )?;
        callbacks.push(Callback {
            key,
            offset,
            condition: 0,
            metadata,
        });
    }
    w.provider(
        graph,
        DEFINITION + 0x300,
        DEFINITION,
        Contract {
            interface: 0x80809ae1,
            instance_interface: 0x80809ae2,
            implementation: 0x80803cf2,
            methods: &[9],
            symbol: "vehicle-flight-thrust-output",
            priority: 0,
        },
    )?;
    put(
        &mut w.payload.0,
        DEFINITION + 0x318,
        &0x811c9dc5u64.to_le_bytes(),
    )?;
    let mut motion = w.finish(native, graph, &[(INSTANCE, 0x80803cf2, 2)], callbacks, json!({
        "owner":"vehicle-motion","instance_class":0x80803cf2u32,"definition_class":0x80803cf1u32,
        "source":source,"forward_metres_per_second":forward,"reverse_metres_per_second":reverse_speed,"acceleration_metres_per_second_squared":acceleration,
        "cruise_thrust_fraction":cruise,"reverse_thrust_fraction":reverse,"boost_thrust_fraction":boost,"boost_to_cruise_ratio":boost / cruise,"cruise_thrust_gain":1.25,"bank_limit_degrees":bank,"response_per_second":response,"response_gain":response_gain,
        "controls":{"pitch":"Relative aim","throttle":"Signed forward and reverse movement input","boost":"Native boost input, LT in the captured controller layout","neutral_thrust":0.},
        "native_maneuvers":maneuvers,
        "hover_boost_release":if maneuvers {Some("Release clears boost without the replaced hover motion clock")}else{None},
        "gameplay_verified":false,
        "limits":["Authored native flight response using source speed, acceleration, cruise and bank limits. Reverse demand preserves the source reverse-to-forward speed ratio against the authored quadratic drag response. A compatible carrier's left and right maneuver states drive roll surfaces with its original action timing and costs. Independent strafing, gravity transition and dive scaling are not reproduced. Live handling needs calibration."]
    }))?;
    motion.inputs = inputs;
    motion.retain_hover_update = maneuvers;
    Ok(motion)
}
