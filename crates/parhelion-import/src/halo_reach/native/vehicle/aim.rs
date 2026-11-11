//! A source gun drawn on a retained carrier's own aim chain, so the driver aims and fires it as the
//! carrier aims its own turret.
//!
//! The stock tank aims its hull's turret bone and cannon bone through its pose definition's aim
//! table. A source whose driver fires a gun with an `aiming` overlay, such as the Scorpion's cannon
//! or a stationary turret's own gun, has that gun's yawing bones drawn on the carrier's yaw bone and
//! its pitching bones on the carrier's pitch bone. Both bones move to the source pivots, the aim clips
//! take the source coverage and pitch limits, and the carrier's `primary_trigger` markers on the
//! pitching chain move to the source muzzle. The carrier's stock turret rows, the tank's missile pod,
//! are removed, since the source's driver fires the source gun alone.
use super::{Scene, assembly::Assembly};
use crate::{
    halo_reach::{
        animation::aim::{self, Turn},
        native::ImportRequest,
    },
    presentation::{Graph, put},
    tiger::{allocation::Row, animation, markers, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

const UNIT: u32 = 0x8080_3C23;
/// Allocation row naming the unit's turret instance array.
const TURRET_ALLOCATION: u32 = 0xA59A_73E6;
const PRIMARY_TRIGGER: u32 = 0x60FE_CDD4;
const TABLE: &str = "vehicle-aim-table";

/// The model whose object fires the driver's weapon, as `projectile::driver_weapon` chooses it: the
/// vehicle's own weapon, else a seatless child's, else for a stationary source or a vehicle without
/// seats its one seated child's. Returns the model and that object's gameplay.
fn driver_gun(
    cache: &crate::halo_reach::cache::Cache,
    scene: &Scene,
) -> Result<Option<(usize, Value)>> {
    let armed = |gameplay: &Value| {
        gameplay["weapons"]
            .as_array()
            .is_some_and(|weapons| weapons.iter().any(|w| !w["tag"].is_null()))
    };
    let seats = |gameplay: &Value| gameplay["seats"].as_array().map_or(0, Vec::len);
    if armed(&scene.object.gameplay) {
        return Ok(Some((0, scene.object.gameplay.clone())));
    }
    let mut children = Vec::new();
    for (index, placed) in scene.models.iter().enumerate().skip(1) {
        let Some(parent) = &placed.parent else {
            continue;
        };
        let object = crate::halo_reach::Object::read(cache, &parent.object)?;
        if armed(&object.gameplay) {
            children.push((index, object.gameplay));
        }
    }
    if let Some(child) = children.iter().find(|(_, gameplay)| seats(gameplay) == 0) {
        return Ok(Some(child.clone()));
    }
    if (super::stationary(scene) || seats(&scene.object.gameplay) == 0)
        && let [(index, gameplay)] = &children[..]
    {
        return Ok(Some((*index, gameplay.clone())));
    }
    Ok(None)
}

/// Native pitch bounds: the overlay's coverage within the controlling seat camera's pitch range.
/// A Reach camera counts pitch up and native aim counts it down. A seat without a range keeps the
/// overlay's coverage.
fn pitch_bounds(scene: &Scene, gun: &Value, coverage: [f32; 2]) -> Result<(Value, [f32; 2])> {
    let seats = gun["seats"]
        .as_array()
        .filter(|seats| !seats.is_empty())
        .or_else(|| scene.object.gameplay["seats"].as_array());
    let seat = seats.and_then(|seats| {
        seats
            .iter()
            .find(|seat| seat["flags"].as_u64().is_some_and(|f| f & 4 != 0))
            .or_else(|| seats.first())
    });
    let range = seat
        .and_then(|seat| seat["camera"]["pitch_range_radians"].as_array())
        .and_then(|range| {
            Some([
                range.first()?.as_f64()? as f32,
                range.get(1)?.as_f64()? as f32,
            ])
        });
    let Some([low, high]) = range.filter(|r| r[1] - r[0] > 1f32.to_radians()) else {
        return Ok((json!({"source":"overlay"}), coverage));
    };
    let bounds = [coverage[0].max(-high), coverage[1].min(-low)];
    if !(bounds[0] < 0. && bounds[1] > 0.) {
        return Ok((
            json!({"source":"overlay","excluded_camera_pitch_range_radians":[low, high]}),
            coverage,
        ));
    }
    Ok((
        json!({"source":"seat camera","seat_marker":seat.map(|s| s["marker"].clone()),"camera_pitch_range_radians":[low, high]}),
        bounds,
    ))
}

/// Before the geometry is drawn: when a retained carrier aims its own turret through an aim table
/// and the source driver's gun has an aiming overlay, record which carrier bone each gun bone
/// rides, and the source pivots and limits, under `bones["aim"]`.
pub(in crate::halo_reach::native) fn presentation(
    native: &mut Reader,
    cache: &crate::halo_reach::cache::Cache,
    scene: &Scene,
    entry: &ImportRequest,
    root: &Path,
    bones: &mut Value,
) -> Result<()> {
    let Some(runtime) = entry.runtime_entity else {
        return Ok(());
    };
    // The tracked and stationary routes keep the tank carrier's turret. Other retained carriers
    // keep their own handling of source guns.
    let tracked = scene.object.gameplay["physics"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["kind"] == "human_tank"));
    // Seated children stay drawn where they are, so a tracked source with gunner seats, such as
    // the Scorpion, still aims its driver's gun here.
    if !bones["source_bone_first"].is_u64()
        || entry.recipe["kind"] != "sparrow"
        || !(tracked || super::stationary(scene))
    {
        return Ok(());
    }
    let Some(carrier) = aim::carrier(native, runtime)? else {
        return Ok(());
    };
    let Some((index, gameplay)) = driver_gun(cache, scene)? else {
        return Ok(());
    };
    let turret = gameplay["physics"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["kind"] == "turret"));
    let node = |field: &str| {
        turret
            .and_then(|row| row[field].as_str())
            .filter(|name| !name.is_empty())
    };
    let source = root
        .parent()
        .context("Import source directory")?
        .join("source");
    let Some(gun) = aim::gun(
        scene,
        &source,
        index,
        [node("physical_yaw_node"), node("physical_pitch_node")],
    )?
    else {
        return Ok(());
    };
    let (bounds, pitch) = pitch_bounds(scene, &gameplay, gun.pitch)?;
    let mapping = gun
        .turns
        .iter()
        .map(|turn| match turn {
            Some(Turn::Yaw) => json!(carrier.yaw_bone),
            Some(Turn::Pitch) => json!(carrier.pitch_bone),
            None => Value::Null,
        })
        .collect::<Vec<_>>();
    bones["aim"] = json!({"model":index,"mapping":mapping,"carrier":carrier,"gun":gun,
        "pitch_bounds":pitch,"pitch_source":bounds,
        "turret_nodes":[node("physical_yaw_node"), node("physical_pitch_node")]});
    Ok(())
}

/// The symbol of each bank clip, by ordinal, from the linked bank's patches.
fn clip_symbols(graph: &Graph, bank: &Payload) -> Result<Vec<String>> {
    let patches = graph.node("motion-bank")?["patches"]
        .as_array()
        .context("Motion bank patches")?
        .clone();
    bank.array(8, 4, Some(0x8080_8F48))?
        .into_iter()
        .map(|row| {
            patches
                .iter()
                .find(|patch| patch["offset"] == row)
                .and_then(|patch| patch["symbol"].as_str())
                .map(str::to_owned)
                .context("Motion bank clip has no linked symbol")
        })
        .collect()
}

/// Remove the unit's stock turret rows, their marker inputs and their allocation row, so the unit
/// creates no child entity.
fn remove_turret_rows(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
) -> Result<Value> {
    let unit = assembly.owner(UNIT)?;
    let payload = &assembly.payloads[&unit];
    let instance = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    let rows = payload.array(definition + 0x160, 144, Some(0x8080_39F7))?;
    if rows.is_empty() {
        return Ok(Value::Null);
    }
    let children = rows
        .iter()
        .map(|&row| Ok(format!("{:08X}", payload.u32(row + 0x18)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut inputs = 0;
    for row in rows {
        inputs += assembly.remove_consumer(unit, u64::try_from(row + 0x38)?);
    }
    let payload = assembly
        .payloads
        .get_mut(&unit)
        .context("Vehicle unit owner")?;
    put(&mut payload.0, instance + 0x6C0, &[0; 16])?;
    put(&mut payload.0, definition + 0x160, &[0; 16])?;
    let (tag, mut layout) = assembly.allocation(native, graph, unit)?;
    Row::replace(&[], &mut layout, 32, &[TURRET_ALLOCATION])?;
    assembly.set_allocation(graph, unit, tag, &layout)?;
    Ok(
        json!({"unit":format!("{unit:08X}"),"removed_child_graphs":children,"removed_marker_inputs":inputs,
        "reason":"The source driver fires the source gun alone"}),
    )
}

/// Move the carrier's `primary_trigger` markers on the pitching chain to the source muzzle, in the
/// bone's frame after its pivot moved.
fn muzzle(
    assembly: &mut Assembly,
    objects: &[[f32; 8]],
    parents: &[i32],
    pitch_bone: usize,
    point: [f32; 3],
) -> Result<Value> {
    let owner = assembly.owner(markers::NATIVE_COMPONENT)?;
    let payload = assembly
        .payloads
        .get_mut(&owner)
        .context("Vehicle marker owner")?;
    let mut moved = Vec::new();
    for row in markers::read_native(payload)? {
        let bone = usize::try_from(row.binding[1])?;
        let mut at = i32::try_from(bone)?;
        let mut riding = false;
        while at >= 0 {
            riding |= at as usize == pitch_bone;
            at = *parents.get(at as usize).context("Marker bone")?;
        }
        if row.name != PRIMARY_TRIGGER || !riding {
            continue;
        }
        ensure!(row.binding[0] == 0, "Muzzle marker binding mode differs");
        let object = objects.get(bone).context("Muzzle marker bone")?;
        let inverse = [-object[0], -object[1], -object[2], object[3]];
        let offset = [
            point[0] - object[4],
            point[1] - object[5],
            point[2] - object[6],
        ];
        let local = rotate(inverse, offset);
        for (axis, value) in local.iter().enumerate() {
            put(
                &mut payload.0,
                row.row + markers::NATIVE.position + axis * 4,
                &value.to_le_bytes(),
            )?;
        }
        moved.push(json!({"bone":bone,"previous":&row.position[..3],"position":local}));
    }
    Ok(json!(moved))
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [
        2. * (y * v[2] - z * v[1]),
        2. * (z * v[0] - x * v[2]),
        2. * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// Apply a gun recorded by [`presentation`] to the retained assembly and its linked motion. Returns
/// the report and, when pivots moved, the skeleton description with the moved inverse binds, which
/// later marker placements must use.
pub(super) fn retained(
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    bones: &Value,
) -> Result<(Value, Option<Value>)> {
    let record = &bones["aim"];
    if record.is_null() {
        return Ok((Value::Null, None));
    }
    let gun: aim::Gun = serde_json::from_value(record["gun"].clone())?;
    let carrier: aim::Carrier = serde_json::from_value(record["carrier"].clone())?;
    let pitch: [f32; 2] = serde_json::from_value(record["pitch_bounds"].clone())?;
    let skeleton_owner = assembly.owner(0x8080_8545)?;
    let controls_owner = assembly.owner(0x8080_8F96)?;
    let pose_owner = assembly.owner(0x8080_348F)?;
    let bank = graph.read("motion-bank")?;
    let symbols = clip_symbols(graph, &bank)?;
    let clips = symbols
        .iter()
        .map(|symbol| graph.read(symbol))
        .collect::<Result<Vec<_>>>()?;
    let pose = &assembly.payloads[&pose_owner];
    let (definition, table) = animation::aim::table_tag(pose)?;
    let table = table.context("Retained carrier pose names no aim table")?;
    ensure!(
        table == carrier.table,
        "Retained carrier aim table differs from its stock one"
    );
    let layers = pose.u32(definition + 0x11C)?;
    let stock = (*native.tag(table, Some(0x8080_348E))?).clone();
    let aimed = aim::apply(
        &aim::Rig {
            skeleton: &assembly.payloads[&skeleton_owner],
            controls: &assembly.payloads[&controls_owner],
            bank: &bank,
            clips: &clips,
            table: &stock,
        },
        &carrier,
        &aim::Target {
            yaw_pivot: gun.yaw_pivot,
            pitch_pivot: gun.pitch_pivot,
            yaw: gun.yaw,
            pitch,
        },
    )?;
    let parents = aim::parents(&aimed.skeleton)?;
    assembly
        .payloads
        .insert(skeleton_owner, aimed.skeleton.clone());
    assembly
        .payloads
        .insert(controls_owner, aimed.controls.clone());
    for (&ordinal, payload) in &aimed.clips {
        let symbol = &symbols[ordinal];
        ensure!(
            graph.node(symbol)?["patches"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "Motion clip {symbol} has references"
        );
        graph.replace(symbol, &payload.0, Vec::new())?;
    }
    let layer_symbol = format!("motion-layers-{layers:08X}");
    if graph.contains(&layer_symbol) {
        let mut payload = graph.read(&layer_symbol)?;
        animation::layer::clip_sizes(&mut payload, &aimed.sizes)?;
        let patches = graph.node(&layer_symbol)?["patches"]
            .as_array()
            .context("Motion layer patches")?
            .clone();
        graph.replace(&layer_symbol, &payload.0, patches)?;
    }
    if let Some(bank) = &aimed.bank {
        let patches = graph.node("motion-bank")?["patches"]
            .as_array()
            .context("Motion bank patches")?
            .clone();
        graph.replace("motion-bank", &bank.0, patches)?;
    }
    graph.add(TABLE, table, &aimed.table.0, None, Vec::new())?;
    assembly.resource(
        pose_owner,
        definition + animation::aim::TABLE_FIELD,
        TABLE.into(),
    )?;
    let muzzle = match gun.muzzle {
        Some(point) => muzzle(
            assembly,
            &aimed.objects,
            &parents,
            carrier.pitch_bone,
            point,
        )?,
        None => Value::Null,
    };
    let rows = remove_turret_rows(native, graph, assembly)?;
    let mut moved = bones.clone();
    let described = moved["bones"]
        .as_array_mut()
        .context("Native skeleton description")?;
    ensure!(
        described.len() == aimed.objects.len(),
        "Native skeleton description differs from the moved skeleton"
    );
    for (entry, object) in described.iter_mut().zip(&aimed.objects) {
        let entry = &mut entry["inverse_object_transform"];
        if entry.is_null() {
            continue;
        }
        let inverse = [-object[0], -object[1], -object[2], object[3]];
        let translation = rotate(inverse, [-object[4], -object[5], -object[6]]);
        *entry = json!([
            inverse[0],
            inverse[1],
            inverse[2],
            inverse[3],
            translation[0],
            translation[1],
            translation[2],
            1.
        ]);
    }
    Ok((
        json!({"source_gun":gun,"carrier":carrier,"native":aimed.report,"pitch_source":record["pitch_source"],
        "muzzle_markers":muzzle,"removed_turret_rows":rows,"gameplay_verified":false,
        "limits":["The source gun's bones ride the carrier's yaw and pitch bones rigidly, so its own firing and reload motion is not drawn. Yaw and pitch rates follow the carrier. Aim direction signs follow the native clip convention and need an in-game check."]}),
        Some(moved),
    ))
}
