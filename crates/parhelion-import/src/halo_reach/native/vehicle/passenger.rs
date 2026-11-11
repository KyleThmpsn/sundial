//! Source seats apart from the driver's that ride as added seats of the carrier's own entity, on a
//! carrier whose unit carries no turret rows, such as the Heavy Pike. Stock transports bind up to
//! ten seat owners on one entity, and the interaction search (`00DE9690`) lists every seat component
//! of an actor. A seat without the control bits takes no controller, so its occupant rides while
//! the driver keeps the vehicle.
use super::{
    Scene,
    assembly::Assembly,
    prompts,
    turret::{self, SEAT_OWNER, TANK_DRIVER_SEAT},
};
use crate::{
    halo_reach::{Mount, native::seat, rig},
    presentation::{Graph, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The stock object type 16 turret's seat owner, a player seat without the driver's control bit.
const TURRET_SEAT: u32 = 0x80B9_EE43;
const SEAT_DEFINITION: u32 = 0x8080_3D7D;
/// Seat flag bits 1 and 2: bit 1 makes the occupant the entity's driver and bit 2 passes the
/// player's controller to the entity (`BBFC40`). A passenger keeps neither.
const CONTROL: u32 = 0b110;
/// The most seats a stock entity binds, on the Fallen and Cabal transports.
const MAX_SEATS: usize = 10;
/// The first stand-in owner of an added seat, replaced by the graph linker before packaging.
const KEY: u32 = 0xFFFF_FF10;
/// Seat definition fields that only the tank's driver seat sets among the stock player seats: an
/// occupant graph at `+0x238`, a scalar at `+0x270`, a scalar at `+0x318` and a `80803D82`
/// resource at `+0x328`. The Pike, Sparrow and turret seats, whose occupants are drawn, leave them
/// empty, and a rider copied from the tank's driver seat was not drawn. A passenger takes the
/// stock turret seat's values.
const OCCUPANT_FIELDS: [usize; 4] = [0x238, 0x270, 0x318, 0x328];

/// One placed marker row: native bone and its bone-local frame.
type Placed = (u32, [f32; 3], [f32; 4]);

/// Write `poses` onto the seat marker row, or onto every entry marker row in turn, of the seat
/// definition at `definition`. Each row keeps its body group and name.
fn place(payload: &mut Payload, definition: usize, entry: bool, poses: &[Placed]) -> Result<usize> {
    let (offset, stride, class) = if entry {
        (0x188, 80, 0x8080_3d6f)
    } else {
        (0x108, 64, 0x8080_3d70)
    };
    let rows = payload.array(definition + offset + 0x50, stride, Some(class))?;
    ensure!(
        !rows.is_empty() && (entry || rows.len() == 1) && !poses.is_empty(),
        "Carrier seat marker rows differ"
    );
    for (index, &row) in rows.iter().enumerate() {
        let (bone, position, orientation) = poses[index % poses.len()];
        put(&mut payload.0, row + 4, &bone.to_le_bytes())?;
        for (at, value) in orientation
            .into_iter()
            .chain(position)
            .chain([1.])
            .enumerate()
        {
            put(&mut payload.0, row + 0x10 + at * 4, &value.to_le_bytes())?;
        }
    }
    Ok(rows.len())
}

/// The seat and entry marker placements of `mount` on the vehicle rig. A seat on an attached child,
/// such as a gunner's on its gun, sits where the vehicle draws that child.
fn markers(scene: &Scene, bones: &Value, mount: &Mount) -> Result<(Placed, Vec<Placed>)> {
    let at = turret::model(scene, mount)?;
    let placed = &scene.models[at].model;
    let frame = turret::placement(scene, at)?;
    let local = |marker: &crate::halo_reach::model::Marker| -> Result<Placed> {
        let (position, orientation) = seat::marker_pose(placed, marker)?;
        let object = turret::pose(&rig::multiply(
            &frame,
            &rig::transform(position, orientation),
        ))?;
        turret::local(bones, turret::carrier_bone(scene, at, marker.bone), object)
    };
    let seat = local(seat::selected_marker(
        placed,
        turret::marker_name(&mount.seat, "marker")?,
    )?)?;
    let mut entries = match mount.seat["entry_marker"].as_str() {
        Some(name) if !name.is_empty() => seat::markers_in(placed, name, &placed.selected)?
            .into_iter()
            .map(local)
            .collect::<Result<Vec<_>>>()?,
        _ => Vec::new(),
    };
    if entries.is_empty() {
        entries.push(seat);
    }
    Ok((seat, entries))
}

/// Add one seat to the carrier for each of `mounts`, copied from its driver seat. Each takes the
/// stock turret seat's flags without the control bits and its pose roles, the private rider role
/// when its source actions translate, the source seat and entry markers, a stock player seat's
/// entry reach, and a Ride prompt. Returns the mount report.
#[expect(
    clippy::too_many_arguments,
    reason = "Seats join the scene, rig, mounts, display name and rider roles"
)]
pub(super) fn attach(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
    mounts: &[&Mount],
    name: &str,
    riders: &BTreeMap<String, u32>,
) -> Result<Value> {
    if mounts.is_empty() {
        return Ok(Value::Null);
    }
    let turret_seat = native.tag(TURRET_SEAT, Some(0x8080_9C36))?;
    let turret_definition = turret_seat.pointer(24)?;
    ensure!(
        turret_seat.u32(turret_seat.pointer(16)? - 4)? == SEAT_OWNER
            && turret_seat.u32(turret_definition - 4)? == SEAT_DEFINITION,
        "Stock turret seat layout differs"
    );
    let turret_flags = turret_seat.u32(turret_definition + 0xe0)?;
    ensure!(
        turret_flags & CONTROL == 0b100,
        "Stock turret seat flags differ"
    );
    let flags = turret_flags & !CONTROL;
    let roles = [
        turret_seat.u32(turret_definition + 0xe8)?,
        turret_seat.u32(turret_definition + 0xec)?,
    ];
    let occupant = OCCUPANT_FIELDS
        .iter()
        .map(|&field| Ok((field, turret_seat.u32(turret_definition + field)?)))
        .collect::<Result<Vec<_>>>()?;
    let donor = native.tag(TANK_DRIVER_SEAT, Some(0x8080_9C36))?;
    let donor_definition = donor.pointer(24)?;
    ensure!(
        donor.u32(donor_definition - 4)? == SEAT_DEFINITION,
        "Stock tank driver seat definition differs"
    );
    let driver = assembly.owner(SEAT_OWNER)?;
    let skipped = mounts
        .iter()
        .skip(MAX_SEATS - 1)
        .map(|mount| mount.seat["marker"].clone())
        .collect::<Vec<_>>();
    let mut keys = Vec::new();
    let mut seats = Vec::new();
    for (index, &mount) in mounts.iter().take(MAX_SEATS - 1).enumerate() {
        let marker = turret::marker_name(&mount.seat, "marker")?;
        let (seat_pose, entry_poses) =
            markers(scene, bones, mount).with_context(|| format!("Source seat {marker}"))?;
        let key = KEY + u32::try_from(index)?;
        let payload = assembly.duplicate(
            graph,
            driver,
            key,
            format!("vehicle-passenger-seat-{index}"),
        )?;
        let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        ensure!(
            payload.u32(definition - 4)? == SEAT_DEFINITION,
            "Carrier driver seat definition differs"
        );
        put(&mut payload.0, definition + 0xe0, &flags.to_le_bytes())?;
        let mut cleared = Vec::new();
        for &(field, value) in &occupant {
            if payload.u32(definition + field)? != value {
                put(&mut payload.0, definition + field, &value.to_le_bytes())?;
                cleared.push(format!("+0x{field:X}"));
            }
        }
        let role = mount.seat["animation"].as_str().unwrap_or_default();
        let pose_role = riders.get(role).copied();
        put(&mut payload.0, definition + 0xe8, &roles[0].to_le_bytes())?;
        put(
            &mut payload.0,
            definition + 0xec,
            &pose_role.unwrap_or(roles[1]).to_le_bytes(),
        )?;
        let seat_rows = place(payload, definition, false, &[seat_pose])?;
        let entry_rows = place(payload, definition, true, &entry_poses)?;
        let reach = seat::entry_reach(payload, definition, &donor, donor_definition, false)?;
        keys.push(key);
        seats.push(json!({"kind":"passenger","source":mount.object.path,
            "source_gunner":mount.seat["flags"].as_u64().is_some_and(|flags| flags & 8 != 0),
            "parent_marker":mount.parent_marker,"seat":marker,"role":role,
            "pose_role":pose_role.map(|private| format!("{private:08X}")),
            "copied_from":format!("{driver:08X}"),"flags":format!("{flags:08X}"),
            "occupant_fields_from_turret_seat":cleared,
            "seat_marker":{"bone":seat_pose.0,"position":seat_pose.1,"rows":seat_rows},
            "entry_markers":entry_poses.len(),"entry_rows":entry_rows,"entry_reach":reach}));
    }
    let prompt = prompts::added(native, graph, assembly, &keys, name, "Ride")?;
    Ok(
        json!({"route":"carrier_seats","seats":seats,"prompt":prompt,
        "skipped_beyond_seat_limit":skipped,"gameplay_verified":false,
        "limits":["The carrier's unit carries no turret rows, so each seat is an added seat of the carrier's own entity. A source gunner's seat rides as a passenger, since a gunner who aims apart from the driver needs a unit of its own.","A passenger keeps the driver seat's camera graph and audio cues."]}),
    )
}
