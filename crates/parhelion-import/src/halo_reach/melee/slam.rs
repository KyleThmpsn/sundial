//! Private radial melee effects on the native sword controller's timed strike path.
mod audio;
mod damage;
mod effect;
mod source;
use crate::{
    halo_reach::{Scene, rig},
    tiger::{projectile::Assets, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use damage::Response;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use source::Source;
use std::path::Path;
use sundial::package_authoring::entity::weapon_component_bindings;

/// The source shape, rather than a weapon hash, qualifies this translation.
pub(in crate::halo_reach) fn prepare(
    native: &mut Reader,
    scene: &Scene,
    behavior: &Value,
    recipe: &Value,
    root: &Path,
    source_audio: bool,
) -> Result<Option<Value>> {
    if scene.object.gameplay["weapon_class"] != "hammer" {
        return Ok(None);
    }
    let source = Source::read(behavior, source_audio)?;
    let metres = scene.metres();
    ensure!(
        metres.is_finite() && metres > 0.,
        "Invalid melee translation scale"
    );
    let item = crate::tiger::shadowkeep::extract(
        native,
        crate::graph::hash(&recipe["donor"], "item_hash")?,
    )?;
    let item_tag = u32::from_str_radix(item["item_tag"].as_str().context("Melee donor item")?, 16)?;
    let donor = crate::tiger::rig::inspect(native, item_tag, false)?;
    let runtime = u32::from_str_radix(
        donor["runtime_entity"]
            .as_str()
            .context("Melee donor runtime")?,
        16,
    )?;
    let entity = native.tag(runtime, Some(0x80809c0f))?;
    let bindings = weapon_component_bindings(&entity.0, 0xcd2bceac).map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Melee blast requires one sword controller")
    };
    ensure!(
        binding.concrete_class == 0x808043d2,
        "Melee donor has another controller"
    );
    let sword_owner = native.tag(binding.owner_tag, Some(0x80809c36))?;
    let digest = hex::encode(Sha256::digest(&sword_owner.0));
    let mut assets = Assets::default();
    damage::emit(
        native,
        &mut assets,
        "sword-powered-contact",
        0x81a6b618,
        source.powered_amount,
        Response::Contact,
    )?;
    damage::emit(
        native,
        &mut assets,
        "sword-depleted-contact",
        0x81a6b618,
        source.depleted_amount,
        Response::Contact,
    )?;
    let health_id = damage::emit(
        native,
        &mut assets,
        "sword-radial-damage",
        0x81a6985f,
        source.blast.amount,
        Response::Damage,
    )?;
    let push_id = damage::emit(
        native,
        &mut assets,
        "sword-recipient-push",
        0x81a69860,
        0.,
        Response::Push(source.push.acceleration * metres),
    )?;
    let mut sounds = crate::presentation::Graph::new(root.to_owned(), Vec::new());
    let (root_id, sound) = effect::build(
        native,
        &mut assets,
        &source,
        metres,
        health_id,
        push_id,
        &mut sounds,
    )?;
    let report = json!({"source_contact":source.powered,"source_depleted":source.depleted,"source_effect":source.effect["tag"],
        "source_damage":source.blast.source,"source_push":source.push.source,"length_scale":metres,
        "source_units_to_metres":rig::METRES_PER_UNIT,"native_push_strength":source.push.acceleration * metres,
        "impact_audio":sound,
        "activation":"Native ammo-gated sword profiles at their accepted strike phase, including misses",
        "falloff":"One inside the source inner radius, linear to zero at the outer radius",
        "retained_native_behavior":["Attack selection, timing, cancellation and ammo costs", "Warmind recipient filters and visual presentation", "Target resistance and difficulty scaling"],
        "limits":["Push strength uses source instantaneous acceleration times the import length scale as a provisional native velocity-delta magnitude. Physical equivalence requires gameplay calibration.",
            "The source exponent falloff, energy percentage, clashes, grenade detonation and source impact particles are not translated. The audio route records its own playback limits."],
        "gameplay_verified":false});
    let mut result = assets.write_in(root, "sword", root_id, report)?;
    result["nodes"]
        .as_array_mut()
        .context("Melee graph nodes")?
        .extend(sounds.nodes);
    result["schema"] = json!(1);
    result["source_owner"] = json!(binding.owner_tag);
    result["source_sha256"] = json!(digest);
    result["powered_contact"] = json!("sword-powered-contact");
    result["depleted_contact"] = json!("sword-depleted-contact");
    Ok(Some(result))
}
