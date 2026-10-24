//! Source wheeled and flight controls assembled as a complete private native
//! vehicle, and source hover tuning written onto a retained carrier.
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
mod prompts;
mod secondary;
mod wing;

use crate::{
    halo_reach::{Scene, native::ImportRequest},
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
    cache: &crate::halo_reach::cache::Cache,
    pages: &mut crate::halo_reach::resource::Pages,
) -> Result<Option<Value>> {
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
            matches!(
                row["kind"].as_str(),
                Some("human_jeep" | "alien_fighter" | "alien_scout")
            )
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return retained(native, graph, scene, bones, entry, cache, pages, None).map(Some);
    }
    ensure!(
        selected.len() == 1 && physics.len() == 1,
        "Vehicle motion translation requires one source implementation"
    );
    let controls = selected[0];
    if controls["kind"] == "alien_scout" {
        return retained(
            native,
            graph,
            scene,
            bones,
            entry,
            cache,
            pages,
            Some(controls),
        )
        .map(Some);
    }
    let mut assembly = assembly::Assembly::read(native, graph)?;
    let driver_targets = attachment::build(&mut assembly, scene, bones)?;
    let lighting = assembly.remove_lights(native)?;
    let physical = assembly.owner(0x80803d4c)?;
    let hover = assembly.owner(0x80803cde)?;
    let activation_priority = assembly.motion_priority(hover)?;
    let body_owner = assembly.owner(0x808092d8)?;
    let body = body::build(
        assembly.payloads.get_mut(&body_owner).unwrap(),
        scene,
        bones,
    )?;
    let collision = collision::build(native, graph, &mut assembly, scene, bones)?;
    let damage = damage::build(native, graph, &mut assembly, scene, bones, cache, pages)?;
    let channels = if controls["kind"] == "alien_fighter" {
        assembly.flight_channels(native, graph)?
    } else {
        Vec::new()
    };
    let maneuvers = channels.len() == 5;
    let (contacts, mut motion) = if controls["kind"] == "human_jeep" {
        (
            contact::build(native, &assembly.payloads[&physical], scene, bones, &body)?,
            motion::build(native, graph, controls, activation_priority, scene.metres())?,
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
            )?,
            flight::build(
                native,
                graph,
                controls,
                activation_priority,
                maneuvers,
                scene.metres(),
            )?,
        )
    };
    assembly.contacts(native, graph, physical, &contacts)?;
    if !contacts.inputs.is_empty() {
        assembly.surface_inputs(physical, &contacts, &channels[2..])?;
    }
    let prompts = prompts::build(native, graph, &mut assembly, name)?;
    let camera = if controls["kind"] == "alien_fighter" {
        Some(camera::build(native, graph, &mut assembly)?)
    } else {
        None
    };
    assembly.motion_inputs(graph, &mut motion, &channels[..channels.len().min(2)])?;
    assembly.add_motion(motion.payload.clone());
    let boost = if controls["kind"] == "alien_fighter" {
        assembly.boost(native, graph, scene, bones, controls, cache, pages, &motion)?
    } else {
        assembly::Trails::default()
    };
    let effects = effects::build(native, graph, &mut assembly, &boost.events)?;
    let wheel_animation = if controls["kind"] == "human_jeep" {
        Some(assembly.wheels(native, graph, scene, bones, &motion)?)
    } else {
        None
    };
    let audio = if controls["kind"] == "human_jeep" && entry.source.audio.is_some() {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(assembly.engine_audio(native, graph, &behavior, &motion)?)
    } else {
        None
    };
    let fire_audio = if controls["kind"] == "alien_fighter"
        && let Some(audio) = &entry.source.audio
    {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(assembly.fire_audio(native, graph, &behavior, cache, audio.fire_bus)?)
    } else {
        None
    };
    assembly.emit(native, graph, runtime, hover, Some(&motion))?;
    Ok(Some(json!({
        "root":ROOT,"source_kind":controls["kind"],"motion":motion.report,
        "contacts":contacts.report,"body":body.report,"collision":collision,"damage":damage,"lighting":lighting,"effects":effects,"boost_trails":boost.report,"prompts":prompts,"wheel_animation":wheel_animation,"audio":audio,"fire_audio":fire_audio,"camera":camera,"driver_targets":driver_targets,"gameplay_verified":false,
        "limits":["The carrier supplies interaction and animation state selection. Passenger behavior and full source effect translation are deferred. Handling, collision, audio lifecycle and flight require gameplay acceptance."]
    })))
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
) -> Result<Value> {
    let mut assembly = assembly::Assembly::read(native, graph)?;
    let lighting = assembly.remove_lights(native)?;
    let body_owner = assembly.owner(0x808092d8)?;
    let body = body::build(
        assembly.payloads.get_mut(&body_owner).unwrap(),
        scene,
        bones,
    )?;
    let collision = collision::build(native, graph, &mut assembly, scene, bones)?;
    let damage = damage::build(native, graph, &mut assembly, scene, bones, cache, pages)?;
    let driver_targets = if bones["source_bone_first"].is_number() {
        attachment::build(&mut assembly, scene, bones)?
    } else {
        Value::Null
    };
    let hover = assembly.owner(0x80803cde)?;
    let motion = hover_controls
        .map(|controls| {
            hover::build(
                assembly.payloads.get_mut(&hover).unwrap(),
                hover,
                controls,
                &scene.object.gameplay,
                scene.metres(),
            )
        })
        .transpose()?;
    let fire_audio = if let Some(audio) = &entry.source.audio {
        let behavior = crate::halo_reach::behavior::read(cache, scene)?;
        Some(assembly.fire_audio(native, graph, &behavior, cache, audio.fire_bus)?)
    } else {
        None
    };
    let effects = effects::build(native, graph, &mut assembly, &Default::default())?;
    assembly.emit(
        native,
        graph,
        entry.runtime_entity.context("Retained vehicle runtime")?,
        hover,
        None,
    )?;
    let mut report = json!({"root":ROOT,"body":body.report,"collision":collision,"damage":damage,"lighting":lighting,"effects":effects,"driver_targets":driver_targets,"fire_audio":fire_audio,"gameplay_verified":false,
        "limits":["Native hover handling and interaction are retained. Source solid and hit shapes follow authored damage conditions. Live impacts, hit routing and clearance require gameplay acceptance."]});
    if let Some(motion) = motion {
        report["source_kind"] = json!("alien_scout");
        report["motion"] = motion;
        report["limits"] = json!([
            "Native hover handling and interaction are retained with source speed targets and acceleration limits. Boost, steering and slides stay native. Source solid and hit shapes follow authored damage conditions. Live driving, impacts, hit routing and clearance require gameplay acceptance."
        ]);
    }
    Ok(report)
}
