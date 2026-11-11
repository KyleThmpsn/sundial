//! Source seats apart from the driver's, and the driver's secondary weapon, ride as their own
//! native type 16 entities on the vehicle unit's turret rows, as the stock tank carries its
//! rocket pod. Every child shares one row allocator.
use super::{Scene, assembly::Assembly, prompts};
use crate::{
    halo_reach::{Mount, native::seat, rig},
    presentation::{Graph, append, put},
    tiger::{allocation::Row as Allocation, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The stock object type 16 turret with one player seat, flags `0002000C`, and its own weapon and
/// unit. Its unit sets no parent link at definition `+0x6B0`, so its occupant controls only it.
pub(super) const SEATED: u32 = 0x80BC_DB2A;
/// The stock tank's rocket pod: a seatless type 16 child whose unit sets `+0x6B0` bit 1, so the
/// parent unit's occupant aims and fires it with the secondary input.
const POD: u32 = 0x80BF_AB8E;
/// The stock tank's unit owner, whose one turret row is the row template.
const TANK_UNIT: u32 = 0x80F2_EFE6;
pub(super) const UNIT: u32 = 0x8080_3C23;
const UNIT_DEFINITION: u32 = 0x8080_3A00;
pub(super) const SEAT_OWNER: u32 = 0x8080_42AD;
/// The stock tank's driver seat owner, a player seat whose entry row reaches 8 m with no limit on
/// the marker's direction. Seated children take its entry reach.
pub(super) const TANK_DRIVER_SEAT: u32 = 0x8157_83FD;
const RENDER_OWNER: u32 = 0x8080_72B8;
const SKELETON: (u32, u32) = (0x8080_8545, 0x8080_8546);
const INSTANCE_ROWS: usize = 0x6c0;
pub(super) const DEFINITION_ROWS: usize = 0x160;
const INSTANCE_ROW: (u32, usize) = (0x8080_39F6, 176);
pub(super) const DEFINITION_ROW: (u32, usize) = (0x8080_39F7, 144);
const MARKER_ROW: (u32, usize) = (0x8080_3E91, 64);
/// Allocation row naming the unit's turret instance array.
const ALLOCATION: u32 = 0xA59A_73E6;
/// The most turret rows a stock unit carries, on the eight-row Fallen and Cabal transports.
const MAX_ROWS: usize = 8;
/// Unit definition slots a stock unarmed unit leaves unconnected: weapon input, weapon update and
/// action component, as on the stock Sparrow's unit.
const WEAPON_SLOTS: [usize; 3] = [0x2d0, 0x2f0, 0x330];
const EMPTY: u32 = 0x811C_9DC5;

/// One child entity a unit carries on a turret row.
pub(super) struct Row {
    /// Root symbol of the child's private entity graph.
    pub entity: String,
    /// Native bone of the vehicle rig the child rides on.
    pub bone: u32,
    /// Bone-local placement of the child's origin.
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    /// Row and marker name hashes.
    pub name: u32,
    pub marker: u32,
}

fn rows(payload: &Payload, field: usize, (class, stride): (u32, usize)) -> Result<Vec<usize>> {
    payload.array(field, stride, Some(class))
}

fn single(rows: Vec<usize>, what: &str) -> Result<usize> {
    ensure!(rows.len() == 1, "Stock turret {what} template differs");
    Ok(rows[0])
}

fn fnv(text: &str) -> u32 {
    text.bytes()
        .fold(EMPTY, |h, b| h.wrapping_mul(0x0100_0193) ^ u32::from(b))
}

/// Append `added` to the unit's turret rows after any it already carries, such as the tank's pod.
/// Existing rows move with their marker rows, consumers and resources. Each row's marker input at
/// definition row `+0x38` takes the skeleton provider of the unit's own marker input. Unbound, the
/// child spawn (A1F640) resolves an unset reference and faults.
pub(super) fn rows_attach(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    added: &[Row],
) -> Result<Value> {
    if added.is_empty() {
        return Ok(json!([]));
    }
    let template = native.tag(TANK_UNIT, Some(0x80809c36))?;
    let template_instance = single(
        rows(
            &template,
            template.pointer(16)? + INSTANCE_ROWS,
            INSTANCE_ROW,
        )?,
        "instance row",
    )?;
    let template_definition = single(
        rows(
            &template,
            template.pointer(24)? + DEFINITION_ROWS,
            DEFINITION_ROW,
        )?,
        "definition row",
    )?;
    let template_marker = single(
        rows(&template, template_definition + 0x78, MARKER_ROW)?,
        "marker",
    )?;
    let unit = assembly.owner(UNIT)?;
    let payload = assembly.payloads.get(&unit).context("Vehicle unit owner")?;
    let instance = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    ensure!(
        payload.u32(instance - 4)? == UNIT && payload.u32(definition - 4)? == UNIT_DEFINITION,
        "Vehicle unit layout differs"
    );
    let old_instances = rows(payload, instance + INSTANCE_ROWS, INSTANCE_ROW)?;
    let old_definitions = rows(payload, definition + DEFINITION_ROWS, DEFINITION_ROW)?;
    ensure!(
        old_instances.len() == old_definitions.len()
            && old_instances.len() + added.len() <= MAX_ROWS,
        "Vehicle unit turret rows differ or exceed the stock maximum of {MAX_ROWS}"
    );
    let mut instance_bytes = Vec::new();
    let mut definition_bytes = Vec::new();
    let mut markers = Vec::new();
    for (&ir, &dr) in old_instances.iter().zip(&old_definitions) {
        instance_bytes.extend_from_slice(&payload.0[ir..ir + INSTANCE_ROW.1]);
        definition_bytes.extend_from_slice(&payload.0[dr..dr + DEFINITION_ROW.1]);
        let mut kept = Vec::new();
        for row in rows(payload, dr + 0x78, MARKER_ROW)? {
            kept.extend_from_slice(&payload.0[row..row + MARKER_ROW.1]);
        }
        markers.push(kept);
    }
    for row in added {
        instance_bytes
            .extend_from_slice(&template.0[template_instance..template_instance + INSTANCE_ROW.1]);
        definition_bytes.extend_from_slice(
            &template.0[template_definition..template_definition + DEFINITION_ROW.1],
        );
        let mut marker = template.0[template_marker..template_marker + MARKER_ROW.1].to_vec();
        put(&mut marker, 4, &row.bone.to_le_bytes())?;
        for (index, value) in row.orientation.into_iter().chain(row.position).enumerate() {
            put(&mut marker, 0x10 + index * 4, &value.to_le_bytes())?;
        }
        put(&mut marker, 0x30, &row.marker.to_le_bytes())?;
        markers.push(marker);
    }
    let payload = assembly
        .payloads
        .get_mut(&unit)
        .context("Vehicle unit owner")?;
    append(
        &mut payload.0,
        instance + INSTANCE_ROWS,
        u64::from(INSTANCE_ROW.0),
        &instance_bytes,
        INSTANCE_ROW.1,
    )?;
    append(
        &mut payload.0,
        definition + DEFINITION_ROWS,
        u64::from(DEFINITION_ROW.0),
        &definition_bytes,
        DEFINITION_ROW.1,
    )?;
    let instances = rows(payload, instance + INSTANCE_ROWS, INSTANCE_ROW)?;
    let definitions = rows(payload, definition + DEFINITION_ROWS, DEFINITION_ROW)?;
    // Reciprocal headers and parent pointers follow the stock row: the instance row, its marker
    // description and its second member each point at their definition and their parent.
    let relative = |from: usize, to: usize| (to as i64 - from as i64).to_le_bytes();
    for ((&ir, &dr), marker) in instances.iter().zip(&definitions).zip(&markers) {
        append(
            &mut payload.0,
            dr + 0x78,
            u64::from(MARKER_ROW.0),
            marker,
            64,
        )?;
        for (at, class, target) in [
            (ir, 0x8080_39F7u32, dr),
            (ir + 0x20, 0x8080_9664, dr + 0x28),
            (ir + 0x40, 0x8080_9BD7, dr + 0x38),
            (dr, 0x8080_39F6, ir),
            (dr + 0x28, 0x8080_9663, ir + 0x20),
            (dr + 0x38, 0x8080_9BD6, ir + 0x40),
        ] {
            put(&mut payload.0, at, &unit.to_le_bytes())?;
            put(&mut payload.0, at + 4, &class.to_le_bytes())?;
            put(&mut payload.0, at + 8, &(target as u64).to_le_bytes())?;
        }
        for (at, parent) in [
            (ir + 0x10, instance),
            (ir + 0x30, ir),
            (ir + 0x50, ir + 0x20),
            (ir + 0x90, instance),
        ] {
            put(&mut payload.0, at, &relative(at, parent))?;
        }
    }
    let kept = old_instances.len();
    for (index, row) in added.iter().enumerate() {
        put(
            &mut payload.0,
            definitions[kept + index] + 0x20,
            &row.name.to_le_bytes(),
        )?;
    }
    let end = payload.0.len();
    put(&mut payload.0, 0, &(end as u64).to_le_bytes())?;
    for index in 0..kept {
        assembly.relocate(
            unit,
            old_definitions[index],
            definitions[index],
            DEFINITION_ROW.1,
        )?;
        assembly.relocate(unit, old_instances[index], instances[index], INSTANCE_ROW.1)?;
    }
    let mut report = Vec::new();
    for (index, row) in added.iter().enumerate() {
        let dr = definitions[kept + index];
        let marker_input = assembly.connect_like(unit, 0x8080_9BD7, u64::try_from(dr + 0x38)?)?;
        assembly.resource(unit, dr + 0x18, row.entity.clone())?;
        report.push(json!({"entity":row.entity,"native_bone":row.bone,"position":row.position,
            "orientation":row.orientation,"instance_row":instances[kept + index],"definition_row":dr,
            "marker_input":marker_input}));
    }
    assembly.runtime_end(unit, end)?;
    let (tag, mut layout) = assembly.allocation(native, graph, unit)?;
    Allocation::replace(
        &[Allocation::named(
            ALLOCATION,
            INSTANCE_ROW.0,
            instances.len(),
            Vec::new(),
        )],
        &mut layout,
        32,
        &[ALLOCATION],
    )?;
    assembly.set_allocation(graph, unit, tag, &layout)?;
    Ok(json!({"unit":format!("{unit:08X}"),"kept_rows":kept,"rows":report}))
}

/// The rotation of a rigid column-major matrix.
fn rotation(m: &[f32; 16]) -> Result<[f32; 4]> {
    let r = |row: usize, column: usize| m[column * 4 + row];
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let q = if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        [
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            0.25 * s,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1. + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.;
        [
            0.25 * s,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        ]
    } else if r(1, 1) > r(2, 2) {
        let s = (1. + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.;
        [
            (r(0, 1) + r(1, 0)) / s,
            0.25 * s,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        ]
    } else {
        let s = (1. + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.;
        [
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            0.25 * s,
            (r(1, 0) - r(0, 1)) / s,
        ]
    };
    rig::quaternion(q)
}

pub(super) fn pose(matrix: &[f32; 16]) -> Result<([f32; 3], [f32; 4])> {
    let position = [matrix[12], matrix[13], matrix[14]];
    ensure!(
        position.iter().all(|v| v.is_finite()),
        "Invalid source seat placement"
    );
    Ok((position, rotation(matrix)?))
}

/// The object-space placement of scene model `index`: each attachment's transform on the bone
/// carrying its parent marker, as the vehicle's drawing places it.
pub(super) fn placement(scene: &Scene, index: usize) -> Result<[f32; 16]> {
    let mut matrix = rig::IDENTITY;
    let mut at = index;
    while let Some(parent) = &scene.models[at].parent {
        let mut hop = parent.transform;
        if let Some(bone) = parent.bone {
            let world = rig::world(&scene.models[parent.model].model.bones)?;
            hop = rig::multiply(world.get(bone).context("Source attachment bone")?, &hop);
        }
        matrix = rig::multiply(&hop, &matrix);
        at = parent.model;
    }
    Ok(matrix)
}

/// The root model bone that carries scene model `index`, or the marker's own bone on the root.
pub(super) fn carrier_bone(scene: &Scene, index: usize, marker: Option<usize>) -> Option<usize> {
    let mut at = index;
    let mut bone = marker;
    while let Some(parent) = &scene.models[at].parent {
        bone = parent.bone;
        at = parent.model;
    }
    bone
}

/// The native vehicle bone and bone-local frame of an object-space source placement.
pub(super) fn local(
    bones: &Value,
    bone: Option<usize>,
    source: ([f32; 3], [f32; 4]),
) -> Result<(u32, [f32; 3], [f32; 4])> {
    let bone = match (bones["source_bone_first"].as_u64(), bone) {
        (Some(first), Some(bone)) => usize::try_from(first)? + bone,
        _ => 0,
    };
    // A static carrier skeleton has no bind transforms, and its single root is the object frame.
    let (position, orientation) = if bones["bones"][bone]["inverse_object_transform"].is_null() {
        ensure!(
            bone == 0 && bones["static"] == true,
            "Vehicle turret row bone has no bind transform"
        );
        source
    } else {
        seat::frame(bones, bone, source)?
    };
    Ok((u32::try_from(bone)?, position, orientation))
}

/// Compose `inverse(frame) * pose` for rigid poses.
fn relative(frame: &[f32; 16], source: ([f32; 3], [f32; 4])) -> Result<([f32; 3], [f32; 4])> {
    pose(&rig::multiply(
        &rig::inverse_rigid(frame),
        &rig::transform(source.0, source.1),
    ))
}

/// The scene model a mount's seat is on: the vehicle, or the child attached at its parent marker.
pub(super) fn model(scene: &Scene, mount: &Mount) -> Result<usize> {
    if mount.parent_marker.is_empty() {
        return Ok(0);
    }
    let found = scene
        .models
        .iter()
        .enumerate()
        .filter(|(_, placed)| {
            placed.parent.as_ref().is_some_and(|parent| {
                parent.model == 0
                    && parent.object == mount.object
                    && parent.marker == mount.parent_marker
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = found[..] else {
        anyhow::bail!(
            "Seated child at {} is absent or ambiguous",
            mount.parent_marker
        );
    };
    Ok(index)
}

pub(super) fn marker_name<'a>(seat: &'a Value, field: &str) -> Result<&'a str> {
    seat[field]
        .as_str()
        .filter(|name| !name.is_empty())
        .with_context(|| format!("Source seat {field} is absent"))
}

/// A private copy of the stock seated turret for one source seat. Its seat and entry markers take
/// the source seat's, on the child's root bone. Its model is not drawn: a gunner's source gun stays
/// drawn on the vehicle, and a passenger has none. A passenger's unit leaves its weapon slots
/// unconnected, as a stock unarmed unit does.
#[expect(
    clippy::too_many_arguments,
    reason = "A seat joins the scene, rig, mount, row index, display name and rider roles"
)]
fn seated(
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    bones: &Value,
    mount: &Mount,
    index: usize,
    name: &str,
    riders: &BTreeMap<String, u32>,
    cache: &crate::halo_reach::cache::Cache,
) -> Result<(Row, Value)> {
    let at = model(scene, mount)?;
    let placed = &scene.models[at].model;
    let seat_marker = seat::selected_marker(placed, marker_name(&mount.seat, "marker")?)?;
    let seat_pose = pose(&rig::multiply(&placement(scene, at)?, &{
        let (position, orientation) = seat::marker_pose(placed, seat_marker)?;
        rig::transform(position, orientation)
    }))?;
    // A gunner's child rides at its own attachment, as the source child does. A passenger's rides
    // at its seat.
    let (frame, bone) = if mount.gunner {
        (placement(scene, at)?, carrier_bone(scene, at, None))
    } else {
        (
            rig::transform(seat_pose.0, seat_pose.1),
            carrier_bone(scene, at, seat_marker.bone),
        )
    };
    let entries = match mount.seat["entry_marker"].as_str() {
        Some(name) if !name.is_empty() => seat::markers_in(placed, name, &placed.selected)?,
        _ => Vec::new(),
    };
    let mut entry_poses = Vec::new();
    for marker in &entries {
        let (position, orientation) = seat::marker_pose(placed, marker)?;
        entry_poses.push(pose(&rig::multiply(
            &placement(scene, at)?,
            &rig::transform(position, orientation),
        ))?);
    }
    if entry_poses.is_empty() {
        entry_poses.push(seat_pose);
    }

    let prefix = format!("vehicle-seat-{index}");
    let mut child = Assembly::stock(native, graph, SEATED, 16, prefix.clone())?;
    let skeleton = child.owner(SKELETON.0)?;
    let skeleton_payload = &child.payloads[&skeleton];
    let skeleton_bones = crate::tiger::rig::decode(
        skeleton_payload,
        skeleton_payload.pointer(24)?,
        SKELETON.1,
        skeleton,
        false,
    )?;
    let owner = child.owner(SEAT_OWNER)?;
    let payload = child.payloads.get_mut(&owner).context("Child seat owner")?;
    let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
    ensure!(
        payload.u32(definition - 4)? == 0x8080_3D7D,
        "Stock seated turret definition differs"
    );
    let local_seat = relative(&frame, seat_pose)?;
    let local_entries = entry_poses
        .iter()
        .map(|&entry| Ok((relative(&frame, entry)?, Some(0))))
        .collect::<Result<Vec<_>>>()?;
    let markers = json!([
        seat::rewrite(
            payload,
            definition,
            &skeleton_bones,
            &[(local_seat, Some(0))],
            false
        )?,
        seat::rewrite(payload, definition, &skeleton_bones, &local_entries, true)?,
    ]);
    // The stock turret's entry rows reach 4 m and require the player in front of the marker, an AI
    // turret's shape. Beside the carrier, whose driver seat reaches 8 m from any side, the
    // interaction search then never offered these seats. They take a stock player seat's reach, and
    // the search's distance weighting picks the seat nearest the player.
    let donor = native.tag(TANK_DRIVER_SEAT, Some(0x8080_9C36))?;
    let donor_definition = donor.pointer(24)?;
    ensure!(
        donor.u32(donor_definition - 4)? == 0x8080_3D7D,
        "Stock tank driver seat definition differs"
    );
    let reach = seat::entry_reach(payload, definition, &donor, donor_definition, true)?;
    let role = mount.seat["animation"].as_str().unwrap_or_default();
    let pose_role = riders.get(role).copied();
    if let Some(private) = pose_role {
        put(&mut payload.0, definition + 0xec, &private.to_le_bytes())?;
    }
    let render = child.owner(RENDER_OWNER)?;
    child.remove_component(render)?;
    let disconnected = if mount.gunner {
        Value::Null
    } else {
        let unit = child.owner(UNIT)?;
        let unit_payload = &child.payloads[&unit];
        let unit_definition = unit_payload.pointer(24)?;
        ensure!(
            unit_payload.u32(unit_definition - 4)? == UNIT_DEFINITION
                && unit_payload.u8(unit_definition + 0x6b0)? == 0,
            "Stock seated turret unit differs"
        );
        let slots = WEAPON_SLOTS
            .iter()
            .map(|&slot| (unit_definition + slot) as u64)
            .collect::<Vec<_>>();
        child.disconnect(unit, &slots)?
    };
    let prompt = if mount.gunner {
        prompts::seat(native, graph, &mut child, &format!("{name} Gun"), "Use")?
    } else {
        prompts::seat(native, graph, &mut child, name, "Ride")?
    };
    // A gunner's barrel fires its source gun's converted projectile in place of the stock turret's.
    // Without a firing sound the barrel patch reads no source behavior.
    let fire = match (mount.gunner, &mount.projectile) {
        (true, Some(section)) => {
            match child.fire_audio(
                native,
                graph,
                &Value::Null,
                cache,
                (None, false),
                Some(section),
            ) {
                Ok(report) => report,
                Err(error) => json!(format!("{error:#}")),
            }
        }
        _ => Value::Null,
    };
    child.emit(native, graph, SEATED, 0, None)?;
    let (bone, position, orientation) = local(bones, bone, pose(&frame)?)?;
    let marker = marker_name(&mount.seat, "marker")?;
    let row = Row {
        entity: format!("{prefix}-runtime"),
        bone,
        position,
        orientation,
        name: if mount.gunner { 0x9F6D_B313 } else { EMPTY },
        marker: fnv(marker),
    };
    Ok((
        row,
        json!({"kind":if mount.gunner { "gunner" } else { "passenger" },"source":mount.object.path,
            "parent_marker":mount.parent_marker,"seat":marker,"role":role,
            "pose_role":pose_role.map(|private| format!("{private:08X}")),
            "template":format!("{SEATED:08X}"),"seat_markers":markers,"entry_reach":reach,"prompt":prompt,
            "disconnected_unit_slots":disconnected,"hidden_model":format!("{render:08X}"),"fire":fire}),
    ))
}

/// A private copy of the stock pod firing the converted secondary projectile, on the source
/// weapon's barrel marker or the vehicle origin. Its model is not drawn, since the source fires
/// from the vehicle's own body. Its unit keeps the parent link, so the driver aims and fires it.
fn secondary_pod(
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    bones: &Value,
    cache: &crate::halo_reach::cache::Cache,
    section: &Value,
) -> Result<(Row, Value)> {
    let prefix = "vehicle-secondary".to_owned();
    let mut child = Assembly::stock(native, graph, POD, 16, prefix.clone())?;
    let unit = child.owner(UNIT)?;
    let unit_payload = &child.payloads[&unit];
    ensure!(
        unit_payload.u8(unit_payload.pointer(24)? + 0x6b0)? == 2,
        "Stock pod unit no longer links to its parent"
    );
    let behavior = crate::halo_reach::behavior::read(cache, scene)?;
    let fire = child.fire_audio(
        native,
        graph,
        &behavior,
        cache,
        (None, false),
        Some(section),
    )?;
    let render = child.owner(RENDER_OWNER)?;
    child.remove_component(render)?;
    child.emit(native, graph, POD, 0, None)?;
    let root = &scene.models[0].model;
    let barrel = section["barrel"]["marker"].as_str().unwrap_or_default();
    let found = if barrel.is_empty() {
        Vec::new()
    } else {
        seat::markers_in(root, barrel, &root.selected)?
    };
    // Several markers of one name are the barrel's alternating muzzles. The pod fires from the
    // first.
    let (source, marker_bone) = match found.first() {
        Some(marker) => (seat::marker_pose(root, marker)?, marker.bone),
        None => (([0.; 3], [0., 0., 0., 1.]), None),
    };
    let (bone, position, orientation) = local(bones, marker_bone, source)?;
    Ok((
        Row {
            entity: format!("{prefix}-runtime"),
            bone,
            position,
            orientation,
            name: 0x9F6D_B313,
            marker: if barrel.is_empty() {
                EMPTY
            } else {
                fnv(barrel)
            },
        },
        json!({"template":format!("{POD:08X}"),"fire":fire,"barrel_marker":barrel,
            "barrel_markers_found":found.len(),"hidden_model":format!("{render:08X}"),
            "aimed_by":"Driver, through the pod unit's parent link at +0x6B0 bit 1",
            "gameplay_verified":false}),
    ))
}

/// Attach every source seat the vehicle's mounts name, then the driver's secondary weapon when the
/// import converted one. Returns the mount report and the secondary weapon report, an object only
/// when the secondary weapon rides on the vehicle, else the reason it does not.
#[expect(
    clippy::too_many_arguments,
    reason = "Seats join the scene, rig, source cache, display name, rider roles and secondary"
)]
pub(super) fn attach(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
    cache: &crate::halo_reach::cache::Cache,
    name: &str,
    riders: &BTreeMap<String, u32>,
    secondary: Option<&Value>,
) -> Result<(Value, Value)> {
    // Rows the unit already carries, such as the tank's pod, and the secondary weapon's row come
    // out of the stock maximum before the seats, which the mounts already order.
    let unit = assembly.owner(UNIT)?;
    // A carrier whose stock unit carries no turret rows, such as the Heavy Pike, was not created
    // with added rows. Its seats ride on its own entity, and it carries no secondary pod. The stock
    // unit decides, since the driver's retained gun may already have replaced the tank's rows.
    let stock = native.tag(unit, Some(0x8080_9C36))?;
    if rows(&stock, stock.pointer(24)? + DEFINITION_ROWS, DEFINITION_ROW)?.is_empty() {
        let mount = super::passenger::attach(
            native,
            graph,
            assembly,
            scene,
            bones,
            &scene.mounts.iter().collect::<Vec<_>>(),
            name,
            riders,
        )
        .context("Carrier seats")?;
        let secondary = secondary.map_or(Value::Null, |_| {
            json!("The carrier's unit carries no turret rows for a secondary pod")
        });
        return Ok((mount, secondary));
    }
    // A passenger needs no unit of its own, and the game never offered a passenger on a turret row
    // beside the carrier's driver seat (Dawn 1.7.2, every child past its entry test and gate). Only
    // a gunner, who aims apart from the driver, takes a turret row. Passengers ride on the carrier.
    let (gunners, passengers): (Vec<&Mount>, Vec<&Mount>) =
        scene.mounts.iter().partition(|mount| mount.gunner);
    let carried = super::passenger::attach(
        native,
        graph,
        assembly,
        scene,
        bones,
        &passengers,
        name,
        riders,
    )
    .context("Carrier seats")?;
    let payload = &assembly.payloads[&unit];
    let existing = rows(payload, payload.pointer(16)? + INSTANCE_ROWS, INSTANCE_ROW)?.len();
    let capacity = MAX_ROWS.saturating_sub(existing + usize::from(secondary.is_some()));
    let skipped = gunners
        .iter()
        .skip(capacity)
        .map(|mount| mount.seat["marker"].clone())
        .collect::<Vec<_>>();
    let mut added = Vec::new();
    let mut seats = Vec::new();
    for (index, mount) in gunners.iter().copied().take(capacity).enumerate() {
        let (row, report) = seated(
            native, graph, scene, bones, mount, index, name, riders, cache,
        )
        .with_context(|| {
            format!(
                "Source seat {}",
                mount.seat["marker"].as_str().unwrap_or_default()
            )
        })?;
        added.push(row);
        seats.push(report);
    }
    let secondary = match secondary {
        Some(section) => match secondary_pod(native, graph, scene, bones, cache, section) {
            Ok((row, report)) => {
                added.push(row);
                report
            }
            Err(error) => json!(format!("{error:#}")),
        },
        None => Value::Null,
    };
    let rows = rows_attach(native, graph, assembly, &added)?;
    let mount = if seats.is_empty() && skipped.is_empty() && carried.is_null() {
        Value::Null
    } else {
        json!({"seats":seats,"rows":rows,"skipped_beyond_row_limit":skipped,"carrier_seats":carried,
            "gameplay_verified":false,
            "limits":["Each gunner seat is a private copy of the stock seated turret with its model hidden. A gunner's source gun stays drawn on the vehicle without aiming. The stock turret's aim stands in for it, and its barrel fires the source gun's converted projectile, until the aim builder moves the gun onto its entity.","The game has not offered a turret-row seat beside the carrier's driver seat."]})
    };
    Ok((mount, secondary))
}
