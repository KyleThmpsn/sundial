//! A stationary turret's gunner seat is the native driver seat.
//!
//! A source with turret physics only has no driver seat. Its one seat, on the object itself or on
//! the gun object attached to it (as the Shade's gun carries the seat), is the seat a player takes
//! to aim and fire the gun. The native carrier's driver aims and fires its hull gun, so that seat
//! becomes the scene's driver seat. A seat on the attached gun keeps its markers, carried through
//! the attachment into the vehicle's own frame, so seat placement, entry and camera read them as
//! the vehicle's markers.
use super::{Scene, stationary};
use crate::halo_reach::{Object, cache::Cache, model::Marker, native::seat, rig};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The source seat flag of the driver.
const DRIVER: u64 = 1 << 2;

/// The rotation of a rigid column-major frame as a unit quaternion `[x, y, z, w]`.
fn rotation(m: &[f32; 16]) -> Result<[f32; 4]> {
    let r = |row: usize, column: usize| f64::from(m[column * 4 + row]);
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        [
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            s / 4.,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1. + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.;
        [
            s / 4.,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        ]
    } else if r(1, 1) > r(2, 2) {
        let s = (1. + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.;
        [
            (r(0, 1) + r(1, 0)) / s,
            s / 4.,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        ]
    } else {
        let s = (1. + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.;
        [
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            s / 4.,
            (r(1, 0) - r(0, 1)) / s,
        ]
    };
    rig::quaternion(q.map(|v| v as f32))
}

/// A marker of an attached model in the root model's object frame, through every attachment.
fn carried(scene: &Scene, model: usize, marker: &Marker) -> Result<Marker> {
    let (position, orientation) = seat::marker_pose(&scene.models[model].model, marker)?;
    let mut frame = rig::transform(position, orientation);
    let mut at = model;
    while let Some(parent) = &scene.models[at].parent {
        frame = rig::multiply(&parent.transform, &frame);
        if let Some(bone) = parent.bone {
            let world = rig::world(&scene.models[parent.model].model.bones)?;
            frame = rig::multiply(world.get(bone).context("Source attachment bone")?, &frame);
        }
        at = parent.model;
    }
    ensure!(at == 0, "Source seat object is not attached to the vehicle");
    let translation = [frame[12], frame[13], frame[14]];
    ensure!(
        translation.iter().all(|v| v.is_finite()),
        "Source carried seat marker is invalid"
    );
    Ok(Marker {
        name: marker.name.clone(),
        region: None,
        permutation: None,
        bone: None,
        translation,
        rotation: rotation(&frame)?,
        scale: marker.scale,
    })
}

/// Make a stationary source's gunner seat its driver seat. Returns the report, or none when the
/// source is not stationary or already has a driver seat.
pub(in crate::halo_reach) fn driver(cache: &Cache, scene: &mut Scene) -> Result<Option<Value>> {
    if !stationary(scene) {
        return Ok(None);
    }
    let seats = scene.object.gameplay["seats"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if seats
        .iter()
        .any(|seat| seat["flags"].as_u64().is_some_and(|f| f & DRIVER != 0))
    {
        return Ok(None);
    }
    let (seat, model) = if let [seat] = &seats[..] {
        (seat.clone(), 0)
    } else {
        ensure!(
            seats.is_empty(),
            "A stationary source with several seats needs an explicit driver"
        );
        let mut found = Vec::new();
        for (index, placed) in scene.models.iter().enumerate().skip(1) {
            let Some(parent) = &placed.parent else {
                continue;
            };
            if parent.model != 0 {
                continue;
            }
            let object = Object::read(cache, &parent.object)?;
            if let Some([seat]) = object.gameplay["seats"].as_array().map(Vec::as_slice) {
                found.push((seat.clone(), index));
            }
        }
        let [(seat, index)] = &found[..] else {
            ensure!(
                found.is_empty(),
                "A stationary source has several seated guns"
            );
            return Ok(None);
        };
        (seat.clone(), *index)
    };
    let mut names = vec![
        seat["marker"].as_str().unwrap_or_default().to_owned(),
        seat["entry_marker"].as_str().unwrap_or_default().to_owned(),
    ];
    if let Some(camera) = seat["camera"]["marker"].as_str() {
        names.push(camera.to_owned());
    }
    let mut carried_markers = Vec::new();
    if model != 0 {
        for name in names.iter().filter(|name| !name.is_empty()) {
            let placed = &scene.models[model].model;
            for marker in seat::markers_in(placed, name, &placed.selected)? {
                carried_markers.push(carried(scene, model, marker)?);
            }
        }
        ensure!(
            !carried_markers.is_empty(),
            "The stationary source seat has no markers on its gun"
        );
        let root = &mut scene.models[0].model;
        for marker in &carried_markers {
            ensure!(
                !root.markers.iter().any(|m| m.name == marker.name),
                "Carried seat marker {} collides with a vehicle marker",
                marker.name
            );
        }
        root.markers.extend(carried_markers.iter().cloned());
    }
    let flags = seat["flags"].as_u64().context("Source seat flags")? | DRIVER;
    let mut driver = seat;
    driver["flags"] = json!(flags);
    scene.object.gameplay["seats"] = json!([driver]);
    Ok(Some(json!({
        "seat_model":model,"markers":names,"carried":carried_markers.len(),"flags":flags,
        "rule":"The stationary source's one gunner seat is the native driver seat"
    })))
}
