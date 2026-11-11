//! Source wheeled and flight controls assembled as a complete private native
//! vehicle, and source hover tuning written onto a retained carrier.
mod aim;
mod assembly;
mod attachment;
mod body;
mod camera;
mod collision;
mod contact;
mod curve;
mod damage;
mod effects;
mod flight;
mod fragment;
mod hover;
mod motion;
mod passenger;
mod prompts;
mod secondary;
mod stationary;
mod turret;
mod wing;

pub(super) use aim::presentation as aim_presentation;
pub(in crate::halo_reach) use assembly::launch;
pub(in crate::halo_reach) use damage::{contact_cue, effect_host};
pub(in crate::halo_reach) use stationary::driver as stationary_driver;

use crate::{
    halo_reach::{Scene, animation::native as source_motion, native::ImportRequest},
    presentation::Graph,
    tiger::reader::Reader,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const METRES: f32 = 3.048;
pub(super) const ROOT: &str = "vehicle-runtime";

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    bones: &Value,
    entry: &ImportRequest,
    (cache, pages): (
        &crate::halo_reach::cache::Cache,
        &mut crate::halo_reach::resource::Pages,
    ),
    projectiles: [Option<&Value>; 2],
) -> Result<Option<Value>> {
    // The driver's primary projectile, and the one the secondary trigger fires.
    let [projectile, secondary] = projectiles;
    let runtime = entry.runtime_entity.context("Vehicle runtime")?;
    let name = entry.recipe["name"]
        .as_str()
        .context("Vehicle display name")?;
    let Some(physics) = scene.object.gameplay["physics"].as_array() else {
        return Ok(None);
    };
    let selected = physics
        .iter()
        .filter(|row| {
            row["kind"].as_str().is_some_and(|kind| {
                matches!(kind, "human_jeep" | "alien_scout") || flight::KINDS.contains(&kind)
            })
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return retained(
            native, graph, scene, bones, entry, cache, pages, None, projectile,
        )
        .map(Some);
    }
    ensure!(
        selected.len() == 1 && physics.len() == 1,
        "Vehicle motion translation requires one source implementation"
    );
    if selected[0]["kind"] == "alien_scout" {
        return retained(
            native,
            graph,
            scene,
            bones,
            entry,
            cache,
            pages,
            Some(selected[0]),
            projectile,
        )
        .map(Some);
    }
    // Every flight kind is flown through the same authored flight response, from the control
    // set its source row maps onto.
    let flies = selected[0]["kind"] != "human_jeep";
    let adapted = if flies {
        let gravity = motion::number(&scene.object.gameplay["contacts"], "gravity_scale")?;
        Some(
            flight::controls(selected[0], gravity, scene.metres())
                .context("Vehicle flight controls")?,
        )
    } else {
        None
    };
    let controls = adapted.as_ref().unwrap_or(selected[0]);
    // A driver weapon on a seatless child, such as the Falcon's chin gun, arms the driver too. Its
    // converted projectile then hosts the launch cue this build writes.
    let armed = fires(cache, &scene.object)? || projectile.is_some();
    let mut assembly = assembly::Assembly::read(native, graph)?;
    let driver_targets =
        attachment::build(&mut assembly, scene, bones).context("Vehicle driver targets")?;
    let lighting = assembly.remove_lights(native)?;
    // A source vehicle whose weapons fire nothing, such as a horn, or whose driver has no weapon,
    // such as a dropship's pilot, gives its driver no weapon.
    let carrier_weapon = if !armed {
        Some(assembly.disarm(native)?)
    } else {
        None
    };
    let physical = assembly.owner(0x80803d4c)?;
    let hover = assembly.owner(0x80803cde)?;
    let activation_priority = assembly.motion_priority(hover)?;
    let body_owner = assembly.owner(0x808092d8)?;
    // Translated wheeled and flight motion carry the source's own mass.
    let body = body::build(
        assembly.payloads.get_mut(&body_owner).unwrap(),
        scene,
        bones,
        1.,
    )
    .context("Vehicle body")?;
    let collision = collision::build(native, graph, &mut assembly, scene, bones, body.mass_scale)
        .context("Vehicle collision")?;
    let damage = damage::build(native, graph, &mut assembly, scene, bones, cache, pages)
        .context("Vehicle damage")?;
    let channels = if flies {
        assembly.flight_channels(native, graph)?
    } else {
        Vec::new()
    };
    let maneuvers = channels.len() == 5;
    let (contacts, mut motion) = if !flies {
        (
            contact::build(
                native,
                &assembly.payloads[&physical],
                scene,
                bones,
                &body,
                false,
                &source_motion::suspension(scene, &graph.root)
                    .context("Vehicle source suspension")?,
            )
            .context("Vehicle wheel contacts")?,
            motion::build(native, graph, controls, activation_priority, scene.metres())
                .context("Vehicle engine and motion")?,
        )
    } else {
        (
            wing::build(
                native,
                &assembly.payloads[&physical],
                scene,
                bones,
                controls,
                &body,
                &channels[2..]
                    .iter()
                    .map(|channel| channel.name)
                    .collect::<Vec<_>>(),
            )
            .context("Vehicle flight surfaces")?,
            flight::build(
                native,
                graph,
                controls,
                activation_priority,
                maneuvers,
                scene.metres(),
            )
            .context("Vehicle flight motion")?,
        )
    };
    assembly.contacts(native, graph, physical, &contacts)?;
    if !contacts.inputs.is_empty() {
        assembly.surface_inputs(physical, &contacts, &channels[2..])?;
    }
    let prompts = prompts::build(native, graph, &mut assembly, name, "Drive")?;
    let camera =
        camera::build(native, graph, &mut assembly, scene, bones).context("Vehicle camera")?;
    assembly.motion_inputs(graph, &mut motion, &channels[..channels.len().min(2)])?;
    assembly.add_motion(motion.payload.clone());
    let boost = if flies {
        assembly.boost(native, graph, scene, bones, controls, cache, pages, &motion)?
    } else {
        assembly::Trails::default()
    };
    let effects = effects::build(native, graph, &mut assembly, &boost.events)?;
    let wheel_animation = if !flies {
        Some(
            assembly
                .wheels(native, graph, scene, bones, &motion, &contacts.suspension)
                .context("Vehicle wheel animation")?,
        )
    } else {
        None
    };
    // A flight row with prop rotation speeds, the vtol's, spins its rotor overlay.
    let rotor_animation =
        if flies && controls["source"]["prop_rotation_cycles_per_second"].is_array() {
            Some(
                assembly
                    .rotor(native, graph, scene, bones, controls, &motion)
                    .context("Vehicle rotor animation")?,
            )
        } else {
            None
        };
    // Wheeled and flight motion both drive the source engine loops on the carrier's engine flow.
    let audio = if entry.source.audio.is_some() {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(
            assembly
                .engine_audio(native, graph, scene, &behavior, Some(&motion))
                .context("Vehicle engine audio")?,
        )
    } else {
        None
    };
    // A translated source projectile replaces the carrier's on any route. Without one, a flight
    // carrier's own projectile takes the source launch motion and sound.
    let fire_audio = if armed && (projectile.is_some() || (flies && entry.source.audio.is_some())) {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(assembly.fire_audio(
            native,
            graph,
            &behavior,
            cache,
            (
                entry.source.audio.as_ref().and_then(|audio| audio.fire_bus),
                entry.source.audio.is_some(),
            ),
            projectile,
        )?)
    } else {
        None
    };
    let (mount, secondary_weapon) = turret::attach(
        native,
        graph,
        &mut assembly,
        scene,
        bones,
        cache,
        name,
        &riders(entry),
        secondary,
    )
    .context("Vehicle mount")?;
    assembly.emit(native, graph, runtime, hover, Some(&motion))?;
    Ok(Some(json!({
        "root":ROOT,"source_kind":controls["kind"],"motion":motion.report,"carrier_weapon":carrier_weapon,"mount":mount,"secondary_weapon":secondary_weapon,
        "contacts":contacts.report,"body":body.report,"collision":collision,"damage":damage,"lighting":lighting,"effects":effects,"boost_trails":boost.report,"prompts":prompts,"wheel_animation":wheel_animation,"rotor_animation":rotor_animation,"audio":audio,"fire_audio":fire_audio,"camera":camera,"driver_targets":driver_targets,"gameplay_verified":false,
        "limits":["The carrier supplies interaction and animation state selection. Full source effect translation is deferred."]
    })))
}

/// Whether the vehicle carries seated children on its unit: every route, except a stationary gun,
/// whose one seat becomes the carrier's own driver seat.
pub(super) fn attaches_mounts(scene: &Scene) -> bool {
    scene.object.gameplay["physics"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty())
        && !stationary(scene)
}

/// Whether the native carrier `entity` creates children from unit turret rows: its unit's stock
/// definition carries at least one. The tank's does. A Pike's unit carries none, and a Pike
/// carrying added rows was not created when summoned, with no fault or log (Dawn 1.7.2,
/// 2026-11-06: a Forklift with no rows spawned, a Mongoose with one seat row did not, while the
/// tank spawned with five). Seats and a secondary pod ride only on a carrier that does.
pub(in crate::halo_reach) fn carries_children(native: &mut Reader, entity: u32) -> Result<bool> {
    let payload = (*native.tag(entity, Some(0x8080_9C0F))?).clone();
    for row in payload.array(16, 12, Some(0x8080_9C04))? {
        let owner = payload.u32(row)?;
        let component = native.tag(owner, None)?;
        let instance = component.pointer(16)?;
        if instance < 4 || component.u32(instance - 4)? != 0x8080_3C23 {
            continue;
        }
        let definition = component.pointer(24)?;
        return Ok(!component
            .array(definition + 0x160, 144, Some(0x8080_39F7))?
            .is_empty());
    }
    Ok(false)
}

/// Record the source seats that ride as seated children. Every child stays drawn on the vehicle.
pub(super) fn detach_mounts(
    cache: &crate::halo_reach::cache::Cache,
    scene: &mut Scene,
) -> Result<()> {
    scene.mounts = mounts(cache, scene)?;
    Ok(())
}

/// Source unit seat flags: invisible, locked, driver, gunner, boarding and invalid for players.
const SEAT_INVISIBLE: u64 = 1 << 0;
const SEAT_LOCKED: u64 = 1 << 1;
const SEAT_DRIVER: u64 = 1 << 2;
const SEAT_GUNNER: u64 = 1 << 3;
const SEAT_BOARDING: u64 = 1 << 10;
const SEAT_INVALID_FOR_PLAYER: u64 = 1 << 14;
/// Source AI seat types 3 and 4 carry small and large cargo, such as a dropship's vehicle.
const CARGO_SEATS: [i64; 2] = [3, 4];

/// Every source seat a player can take apart from the driver's: the vehicle's own passenger seats,
/// the seats of an unarmed child such as a troop bed, and one gunner seat for each child with
/// turret physics and a weapon that fires. Hijack seats, locked or invisible seats, cargo seats
/// and seats whose marker the selected variant lacks are left out, and one marker holds one seat.
/// Gunners come first, then the passenger seats the source allows players, then the rest.
pub(super) fn mounts(
    cache: &crate::halo_reach::cache::Cache,
    scene: &Scene,
) -> Result<Vec<crate::halo_reach::Mount>> {
    if !attaches_mounts(scene) {
        return Ok(Vec::new());
    }
    let mut gunners = Vec::new();
    let mut passengers = Vec::new();
    seats(
        &scene.object.tag,
        &scene.object.gameplay,
        "",
        &scene.models[0].model,
        false,
        &mut passengers,
    )?;
    for placed in scene.models.iter().skip(1) {
        let Some(parent) = &placed.parent else {
            continue;
        };
        if parent.model != 0 {
            continue;
        }
        let object = crate::halo_reach::Object::read(cache, &parent.object)?;
        let turret = object.gameplay["physics"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["kind"] == "turret"));
        let armed = turret && fires(cache, &object)?;
        let mut found = Vec::new();
        seats(
            &parent.object,
            &object.gameplay,
            &parent.marker,
            &placed.model,
            armed,
            &mut found,
        )?;
        let mut gunner = false;
        for mount in found {
            if mount.gunner && !gunner {
                gunner = true;
                gunners.push(mount);
            } else {
                passengers.push(crate::halo_reach::Mount {
                    gunner: false,
                    ..mount
                });
            }
        }
    }
    // A stable sort keeps source order within each group.
    passengers.sort_by_key(|mount| {
        mount.seat["flags"]
            .as_u64()
            .is_some_and(|flags| flags & SEAT_INVALID_FOR_PLAYER != 0)
    });
    gunners.extend(passengers);
    Ok(gunners)
}

fn seats(
    object: &crate::halo_reach::cache::Tag,
    gameplay: &Value,
    parent_marker: &str,
    model: &crate::halo_reach::model::Model,
    armed: bool,
    found: &mut Vec<crate::halo_reach::Mount>,
) -> Result<()> {
    let mut markers = std::collections::BTreeSet::new();
    for seat in gameplay["seats"].as_array().into_iter().flatten() {
        let flags = seat["flags"].as_u64().context("Source seat flags")?;
        if flags & (SEAT_INVISIBLE | SEAT_LOCKED | SEAT_DRIVER | SEAT_BOARDING) != 0
            || seat["ai_type"]
                .as_i64()
                .is_some_and(|kind| CARGO_SEATS.contains(&kind))
        {
            continue;
        }
        // A gunner seat of the vehicle itself works the vehicle's own weapon, which the driver
        // carries.
        let gunner = flags & SEAT_GUNNER != 0;
        if gunner && !armed {
            continue;
        }
        let Some(marker) = seat["marker"].as_str().filter(|name| !name.is_empty()) else {
            continue;
        };
        if super::seat::markers_in(model, marker, &model.selected)?.len() != 1
            || !markers.insert(marker.to_owned())
        {
            continue;
        }
        found.push(crate::halo_reach::Mount {
            object: object.clone(),
            parent_marker: parent_marker.to_owned(),
            seat: seat.clone(),
            gunner,
            projectile: None,
        });
    }
    Ok(())
}

/// Whether the source has only turret physics, as a stationary gun emplacement such as a Shade: no
/// drive, no contact points, and a body that turns only its yaw and pitch nodes.
pub(super) fn stationary(scene: &Scene) -> bool {
    scene.object.gameplay["physics"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty() && rows.iter().all(|row| row["kind"] == "turret"))
}

/// The private rider role of each source seat role whose actions the character profile translates.
fn riders(entry: &ImportRequest) -> std::collections::BTreeMap<String, u32> {
    entry
        .character
        .iter()
        .flat_map(|options| &options.passengers)
        .map(|passenger| {
            (
                passenger.role.clone(),
                crate::halo_reach::animation::character::passenger_role(
                    &entry.slug,
                    &passenger.role,
                ),
            )
        })
        .collect()
}

/// Whether any of a source object's own weapons has a barrel that fires a projectile.
fn fires(
    cache: &crate::halo_reach::cache::Cache,
    object: &crate::halo_reach::Object,
) -> Result<bool> {
    for weapon in object.gameplay["weapons"]
        .as_array()
        .context("Source object weapons")?
    {
        if weapon["tag"].is_null() {
            continue;
        }
        let tag: crate::halo_reach::cache::Tag = serde_json::from_value(weapon["tag"].clone())?;
        let object = crate::halo_reach::Object::read(cache, &tag)?;
        if object.gameplay["barrels"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|barrel| !barrel["projectile"].is_null())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Keep the carrier's native hover motion. An `alien_scout` source row tunes that
/// motion's speed targets and acceleration limits before the assembly is emitted.
#[expect(
    clippy::too_many_arguments,
    reason = "The retained assembly joins source geometry, damage, audio and hover tuning"
)]
fn retained(
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    bones: &Value,
    entry: &ImportRequest,
    cache: &crate::halo_reach::cache::Cache,
    pages: &mut crate::halo_reach::resource::Pages,
    hover_controls: Option<&Value>,
    projectile: Option<&Value>,
) -> Result<Value> {
    let mut assembly = assembly::Assembly::read(native, graph)?;
    let lighting = assembly.remove_lights(native)?;
    let body_owner = assembly.owner(0x808092d8)?;
    // A tracked source keeps the carrier's tread drive on its own tread points, so it rides at
    // its own height on its own footprint. That drive is tuned to the carrier's mass, so the
    // source's parts take it (`body::native_mass_scale`).
    // A stationary turret source rests on its collision with no tire rows on the same carrier.
    let tracked = scene.object.gameplay["physics"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["kind"] == "human_tank"));
    let stationary = stationary(scene);
    let mass_scale = if tracked && !stationary {
        body::native_mass_scale(&assembly.payloads[&body_owner], scene)?
    } else {
        1.
    };
    let body = body::build(
        assembly.payloads.get_mut(&body_owner).unwrap(),
        scene,
        bones,
        mass_scale,
    )?;
    let collision = collision::build(native, graph, &mut assembly, scene, bones, body.mass_scale)
        .context("Vehicle collision")?;
    let damage = damage::build(native, graph, &mut assembly, scene, bones, cache, pages)
        .context("Vehicle damage")?;
    let driver_targets = if bones["source_bone_first"].is_number() {
        attachment::build(&mut assembly, scene, bones)?
    } else {
        Value::Null
    };
    let contacts = if tracked || stationary {
        let physical = assembly.owner(0x80803d4c)?;
        let contacts = if stationary {
            contact::stationary(native, &assembly.payloads[&physical], scene)
                .context("Stationary vehicle contacts")?
        } else {
            contact::build(
                native,
                &assembly.payloads[&physical],
                scene,
                bones,
                &body,
                true,
                &source_motion::suspension(scene, &graph.root)
                    .context("Vehicle source suspension")?,
            )
            .context("Vehicle track contacts")?
        };
        assembly.contacts(native, graph, physical, &contacts)?;
        Some(contacts.report)
    } else {
        None
    };
    // Only hover tuning needs the carrier's hover motion. A carrier with another native drive,
    // such as the tank's treads, keeps it as it is.
    let motion = hover_controls
        .map(|controls| -> Result<Value> {
            let hover = assembly.owner(0x80803cde)?;
            hover::build(
                assembly.payloads.get_mut(&hover).unwrap(),
                hover,
                controls,
                &scene.object.gameplay,
                scene.metres(),
            )
        })
        .transpose()?;
    // The retained hover or tread drive pitches the source engine loops. A source without them,
    // such as a stationary gun, takes the carrier's engine flow away.
    let audio = if entry.source.audio.is_some() {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(
            assembly
                .engine_audio(native, graph, scene, &behavior, None)
                .context("Vehicle engine audio")?,
        )
    } else {
        None
    };
    let fire_audio = if entry.source.audio.is_some() || projectile.is_some() {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(assembly.fire_audio(
            native,
            graph,
            &behavior,
            cache,
            (
                entry.source.audio.as_ref().and_then(|audio| audio.fire_bus),
                entry.source.audio.is_some(),
            ),
            projectile,
        )?)
    } else {
        None
    };
    let effects = effects::build(native, graph, &mut assembly, &Default::default())?;
    // The source driver's gun takes the carrier's own aim chain and replaces its stock turret
    // rows. Later marker placements read the moved pivots, and source mount rows follow.
    let (gun, moved) =
        aim::retained(native, graph, &mut assembly, bones).context("Vehicle gun aim")?;
    let bones = moved.as_ref().unwrap_or(bones);
    let camera =
        camera::build(native, graph, &mut assembly, scene, bones).context("Vehicle camera")?;
    let name = entry.recipe["name"]
        .as_str()
        .context("Vehicle display name")?;
    // The carrier's own prompts would name it, such as the tank's Drake.
    let prompts = prompts::build(
        native,
        graph,
        &mut assembly,
        name,
        if stationary { "Use" } else { "Drive" },
    )
    .context("Vehicle prompts")?;
    // Source seats apart from the driver's ride on the carrier unit after any rows it carries.
    let (mount, _) = turret::attach(
        native,
        graph,
        &mut assembly,
        scene,
        bones,
        cache,
        name,
        &riders(entry),
        None,
    )
    .context("Vehicle mount")?;
    // Without translated motion, emission keeps the carrier entity as its presentation template
    // and reads no hover owner.
    assembly.emit(
        native,
        graph,
        entry.runtime_entity.context("Retained vehicle runtime")?,
        0,
        None,
    )?;
    let mut report = json!({"root":ROOT,"body":body.report,"collision":collision,"damage":damage,"lighting":lighting,"effects":effects,"driver_targets":driver_targets,"fire_audio":fire_audio,"gameplay_verified":false,
        "limits":["The carrier's native handling and interaction are retained. Source solid and hit shapes follow authored damage conditions."]});
    report["camera"] = camera;
    report["prompts"] = prompts;
    report["mount"] = mount;
    report["gun"] = gun;
    report["audio"] = json!(audio);
    if let Some(contacts) = contacts {
        report["source_kind"] = json!(if stationary { "turret" } else { "human_tank" });
        report["contacts"] = contacts;
        report["limits"] = json!([if stationary {
            "The source rests on its collision with no tire rows, so the carrier's drive cannot move it. Its driver aims and fires the source gun. Source solid and hit shapes follow authored damage conditions."
        } else {
            "The carrier's native tread drive and interaction are retained on the source tread points, suspension and tire friction. Source differential steering, engine and gears are not translated. Source solid and hit shapes follow authored damage conditions."
        }]);
    }
    if let Some(motion) = motion {
        report["source_kind"] = json!("alien_scout");
        report["motion"] = motion;
        report["limits"] = json!([
            "Native hover handling and interaction are retained with source speed targets and acceleration limits. Boost, steering and slides stay native. Source solid and hit shapes follow authored damage conditions."
        ]);
    }
    Ok(report)
}
