//! The driver seat's secondary entity, named at seat definition `+0x248`. It holds the seat camera
//! sequence and, on a carrier whose maneuvers fire, the gates of the carrier's own maneuver shot.
//! The vehicle takes a private copy when it follows a source camera track or when its effect flow
//! creates the source secondary projectile in that shot's place.
use super::{
    Scene,
    assembly::{self, Assembly},
    bundle::Bundle,
    camera,
};
use crate::{presentation::Graph, tiger::reader::Reader};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// The seat camera report and, when the carrier's own maneuver shot was closed, its report.
pub(super) struct Driver {
    pub camera: Value,
    pub carrier_shot: Value,
}

/// Copies the driver's secondary entity when the source has a camera track for it or `created`
/// says the vehicle's effect flow creates the source secondary projectile.
pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
    created: bool,
) -> Result<Driver> {
    let track = camera::source(scene)?;
    let seated = !camera::owners(assembly, 0x808042ad).is_empty();
    let retained = if track.is_none() {
        Some(
            "The source driver seat has no third-person camera track, so the carrier camera is kept.",
        )
    } else if !seated {
        Some("The carrier has no driver seat camera to follow the source track.")
    } else {
        None
    };
    let kept = |limit: &str| json!({"retained":true,"limits":[limit]});
    if !seated || (track.is_none() && !created) {
        return Ok(Driver {
            camera: kept(retained.unwrap_or_default()),
            carrier_shot: Value::Null,
        });
    }
    let driver = assembly.owner(0x808042ad)?;
    let payload = &assembly.payloads[&driver];
    let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
    ensure!(
        payload.u32(definition - 4)? == 0x80803d7d,
        "Driver seat owner differs"
    );
    let secondary = payload.u32(definition + 0x248)?;
    let mut bundle = Bundle::read(native, secondary)?;
    let camera = match (track, retained) {
        (Some(track), _) => {
            camera::rails(native, graph, assembly, scene, bones, &mut bundle, track)?
        }
        (None, limit) => kept(limit.unwrap_or_default()),
    };
    let carrier_shot = if created {
        assembly::close_carrier_shot(&mut bundle)?
    } else {
        Value::Null
    };
    let symbol = bundle.emit(graph, "vehicle-driver")?;
    assembly.resource(driver, definition + 0x248, symbol)?;
    Ok(Driver {
        camera,
        carrier_shot,
    })
}
