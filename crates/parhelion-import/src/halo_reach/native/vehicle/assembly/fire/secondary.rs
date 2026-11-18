//! Secondary fire on a carrier whose unit takes no weapon child: the vehicle's effect flow
//! creates the driver's secondary projectile as the carrier's maneuver states become active.
//!
//! The Heavy Pike fires its own maneuver shot the same way. Its driver seat's secondary entity
//! `80C0C30C` holds a flow whose range gates on the maneuver states each create projectile
//! `80C0C188` with two particle events and a sound. The hover update `00EB3310` sets a maneuver
//! state on a press, holds it until the clock at instance `+0x384` reaches the action's duration
//! and takes no other press meanwhile. Hover motion `00EB7D60` advances that clock, and the flight
//! route replaces that motion, so a state stays set after one press. The update itself only adds
//! the profile's regeneration to the energy at `+0x364`, which is zero on the stock profiles.
use super::*;
use crate::{
    halo_reach::{Scene, native::seat},
    tiger::{markers, sequence},
};
use std::collections::BTreeSet;

/// The bytecode of a numeric program that returns its one constant row.
const CONSTANT_PROGRAM: [u8; 4] = [0x34, 0, 0x3e, 0];
/// The share of the carrier's maneuver energy one shot takes. Action entry `00EA4450` needs more
/// energy than the cost, and the pool holds at most one, so a shot needs a nearly full pool.
const SHOT_ENERGY: f32 = 0.99;
/// The shortest recovery the energy pool holds, for a source barrel that sets none.
const SHORTEST_RECOVERY: f32 = 0.1;

/// The constant row of the numeric program whose definition prefix of `class` is at `at`.
fn constant(p: &Payload, owner: u32, class: u32, at: usize) -> Result<usize> {
    ensure!(
        p.u32(at)? == owner && p.u32(at + 4)? == class,
        "Maneuver numeric program differs"
    );
    let code = p.array(at + 0x10, 1, Some(0x80800009))?;
    let rows = p.array(at + 0x20, 16, Some(0x80800090))?;
    ensure!(
        code.len() == CONSTANT_PROGRAM.len()
            && p.0[code[0]..code[0] + CONSTANT_PROGRAM.len()] == CONSTANT_PROGRAM
            && rows.len() == 1,
        "Maneuver numeric program is not a constant"
    );
    Ok(rows[0])
}

/// Closes the carrier's own maneuver shot in `bundle`, the private copy of its driver seat's
/// secondary entity: every range gate whose children create an object takes an empty range, so
/// neither the carrier's projectile nor the particles and sound beside it play.
pub(in crate::halo_reach::native::vehicle) fn close_carrier_shot(
    bundle: &mut Bundle,
) -> Result<Value> {
    let mut closed = Vec::new();
    for (&owner, payload) in &mut bundle.owners {
        let sequence = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        if payload.u32(sequence - 4)? != 0x808084e9 {
            continue;
        }
        let controls = payload.array(sequence + 0x158, 24, Some(0x808093e6))?;
        let mut gates = BTreeSet::new();
        for row in payload.array(sequence + 0x168, 24, Some(0x808093e6))? {
            let event = payload.pointer(row + 16)?;
            if payload.u32(event - 4)? != 0x80808881 {
                continue;
            }
            // A node's parent selector holds the control domain in its low word and the
            // control's ordinal in its high word.
            let parent = payload.u32(event + 0x14)?;
            ensure!(
                parent & 0xffff == 0,
                "Carrier shot creation is not a control's child"
            );
            let control = payload.pointer(
                *controls
                    .get((parent >> 16) as usize)
                    .context("Carrier shot creation parent is outside its flow")?
                    + 16,
            )?;
            ensure!(
                payload.u32(control - 4)? == 0x808093cd,
                "Carrier shot creation is not under a range gate"
            );
            gates.insert(control);
        }
        for gate in gates {
            let bounds = [payload.f32(gate + 0x58)?, payload.f32(gate + 0x5c)?];
            // The gate accepts `minimum <= value < maximum`, so equal bounds accept nothing.
            for field in [0x58, 0x5c] {
                put(&mut payload.0, gate + field, &f32::MAX.to_le_bytes())?;
            }
            closed
                .push(json!({"owner":format!("{owner:08X}"),"gate":gate,"previous_bounds":bounds}));
        }
    }
    Ok(json!({"closed_gates":closed,
        "limits":["The carrier's own maneuver shot, its muzzle particles and its sound are closed in the private driver entity. A carrier that fires that shot another way keeps it."]}))
}

impl Assembly {
    /// Fits the carrier's maneuver to firing on a route without hover motion. The action's
    /// duration becomes zero, so the hover update clears a state on the update after it sets it
    /// and each press is one pulse. The source recovery moves onto the maneuver energy: a shot
    /// takes nearly the whole pool and the profiles regenerate it over `recovery` seconds. Boost
    /// costs no energy, as the source boost has no meter.
    pub(in crate::halo_reach::native::vehicle::assembly) fn maneuver_fire(
        &mut self,
        recovery: f32,
    ) -> Result<Value> {
        let owner = self.owner(0x80803cde)?;
        let hover = self
            .payloads
            .get_mut(&owner)
            .context("Maneuver hover owner")?;
        let definition = usize::try_from(hover.u64(hover.pointer(16)? + 8)?)?;
        ensure!(
            hover.u32(definition - 4)? == 0x80803171 && hover.u64(definition + 0x9b8)? != 0,
            "Maneuver carrier hover definition differs"
        );
        let action = hover.pointer(definition + 0x9b8)?;
        let duration = hover.f32(action + 0x70)?;
        ensure!(
            hover.u32(action - 4)? == 0x80803cce && duration.is_finite() && duration >= 0.,
            "Maneuver action differs"
        );
        let cost = constant(hover, owner, 0x808089f5, action + 0x10)?;
        let mut profiles = Vec::new();
        for profile in [0x6c0, 0x800] {
            let at = definition + profile;
            ensure!(
                hover.u32(at)? == owner && hover.u32(at + 4)? == 0x80803cc5,
                "Maneuver carrier boost profile differs"
            );
            profiles.push((at, constant(hover, owner, 0x808089f7, at + 0x88)?));
        }
        let recovery = recovery.max(SHORTEST_RECOVERY);
        // A shot from a full pool leaves `1 - SHOT_ENERGY`, and the next needs `SHOT_ENERGY`.
        let rate = (2. * SHOT_ENERGY - 1.) / recovery;
        put(&mut hover.0, action + 0x70, &0f32.to_le_bytes())?;
        for lane in 0..4 {
            put(&mut hover.0, cost + lane * 4, &SHOT_ENERGY.to_le_bytes())?;
        }
        for &(profile, regeneration) in &profiles {
            put(&mut hover.0, profile + 0x74, &0f32.to_le_bytes())?;
            for lane in 0..4 {
                put(&mut hover.0, regeneration + lane * 4, &rate.to_le_bytes())?;
            }
        }
        Ok(
            json!({"owner":format!("{owner:08X}"),"action":action,"previous_duration_seconds":duration,
            "shot_energy":SHOT_ENERGY,"energy_per_second":rate,"recovery_seconds":recovery,
            "limits":["Each press is one maneuver pulse and one shot. The maneuver energy holds the source recovery, so a press before it refills does nothing. That the hover update regenerates energy in seconds, the pulse reaches the flow on every press and boost needs no energy are not established in game."]}),
        )
    }
}

/// The converted secondary projectile section, and the bank channels of the carrier's left and
/// right maneuver states. Reach fires it from the driver weapon's secondary trigger. The
/// carrier's maneuver inputs are the inputs its driver has free, so each state creates the
/// projectile as it becomes active.
pub(in crate::halo_reach::native::vehicle) struct Secondary<'a> {
    pub section: &'a Value,
    pub states: &'a [Channel],
}

/// The marker the flow creates the projectile at, the projectile references of its creation
/// events and the secondary weapon report.
pub(in crate::halo_reach::native::vehicle::assembly) struct Written {
    pub marker: markers::Marker,
    pub resources: Vec<(usize, String)>,
    pub report: Value,
}

/// The flow's creation controls: one gate per maneuver state, whose flow inputs start at
/// `first_state`, each holding the source barrel's fire recovery after it creates.
pub(in crate::halo_reach::native::vehicle::assembly) fn creation(
    secondary: &Secondary<'_>,
    first_state: u32,
) -> Result<flow::Creation> {
    let recovery = secondary.section["fire_recovery_seconds"]
        .as_f64()
        .context("Secondary barrel fire recovery")? as f32;
    Ok(flow::Creation {
        locator: super::super::super::damage::hash("source/secondary/barrel"),
        inputs: (first_state..first_state + u32::try_from(secondary.states.len())?).collect(),
        recovery,
    })
}

/// Writes the creation events of `flow` and places their marker. `first` is the first source bone
/// of the vehicle rig and `locator` the flow locator `creation` declared.
pub(in crate::halo_reach::native::vehicle::assembly) fn write(
    flow: &mut flow::Flow,
    secondary: &Secondary<'_>,
    creation: &flow::Creation,
    scene: &Scene,
    bones: &Value,
    first: usize,
    locator: u32,
) -> Result<Written> {
    let section = secondary.section;
    let root = section["root"]
        .as_str()
        .filter(|symbol| !symbol.is_empty())
        .context("Secondary projectile root")?;
    // The flow launches the projectile along the barrel marker's forward axis at its source
    // launch speed. No Barrel launches it, so its conversion keeps no speed curve.
    let speed = section["conversion"]["source_behavior"]["initial_velocity"]
        .as_f64()
        .context("Secondary projectile launch speed")? as f32
        * scene.metres();
    ensure!(
        speed.is_finite() && speed > 0.,
        "Secondary projectile has no launch speed"
    );
    let model = &scene.models[0].model;
    let barrel = section["barrel_marker"].as_str().unwrap_or_default();
    let found = if barrel.is_empty() {
        Vec::new()
    } else {
        seat::markers_in(model, barrel, &model.selected)?
    };
    // Several markers of one name are the barrel's alternating muzzles. The flow creates from the
    // first, and from the vehicle origin when the source names none.
    let (world, bone) = match found.first() {
        Some(marker) => (
            seat::marker_pose(model, marker)?,
            marker.bone.map_or(0, |bone| first + bone),
        ),
        None => (([0.; 3], [0., 0., 0., 1.]), 0),
    };
    let (position, orientation) = seat::frame(bones, bone, world)?;
    let mut resources = Vec::new();
    for &event in &flow.creations {
        let field = sequence::spawn::write(
            &mut flow.payload,
            event,
            &sequence::spawn::Parameters {
                locator,
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
                scale: [1.; 2],
                speed: [speed; 2],
                cone: 0.,
                angular_min: [0.; 3],
                angular_max: [0.; 3],
            },
        )?;
        resources.push((field, root.to_owned()));
    }
    let states = secondary
        .states
        .iter()
        .map(|channel| format!("{:08X}", channel.name))
        .collect::<Vec<_>>();
    Ok(Written {
        marker: markers::Marker {
            name: creation.locator,
            binding: [markers::EFFECT_GROUP, u32::try_from(bone)?],
            position: [position[0], position[1], position[2], 1.],
            orientation,
            row: 0,
        },
        resources,
        report: json!({"route":"effect_flow_creation","owner":format!("{:08X}", flow::OWNER),
            "projectile":root,"source_weapon":section["vehicle_weapon"],
            "source_trigger_input":section["trigger_input"],
            "barrel_marker":barrel,"barrel_markers_found":found.len(),
            "marker":format!("{:08X}", creation.locator),"bone":bone,"position":position,
            "locator":locator,"launch_speed_metres_per_second":speed,
            "recovery_seconds":creation.recovery,"state_channels":states,
            "events":flow.creations,"gameplay_verified":false,
            "limits":["The carrier's left and right maneuver states each create the projectile as they become active, in place of the source secondary trigger, and the maneuver no longer rolls the body. The carrier's maneuver energy holds the source fire recovery, as the maneuver report says. The projectile flies along the barrel marker's forward axis, not the driver's aim. The source firing effect and sound, damage credit to the driver and the projectile's clearance of the vehicle that creates it are not established."]}),
    })
}
