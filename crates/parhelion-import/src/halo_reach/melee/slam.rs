//! Private radial melee effects on the native sword controller's timed strike path.
mod audio;
mod damage;
mod effect;
mod source;
mod surface;
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

const AIR_LUNGE: u32 = 0x054A_16BE;
const AIR_SLAM: u32 = 0x7843_564F;

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
        Response::Push(rig::push_strength(source.push.acceleration, metres)),
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
    // The blast plays the source hit on every strike. Contact hits also take it on every surface.
    let surface = if source.sound.is_some() {
        Some(surface::build(
            native,
            &sword_owner,
            &mut sounds,
            "sword-impact-cue",
        )?)
    } else {
        None
    };
    let report = json!({"source_contact":source.powered,"source_depleted":source.depleted,"source_effect":source.effect["tag"],
        "source_damage":source.blast.source,"source_push":source.push.source,"length_scale":metres,
        "source_units_to_metres":rig::METRES_PER_UNIT,"native_push_strength":rig::push_strength(source.push.acceleration, metres),
        "impact_audio":sound,"surface_audio":surface.as_ref().map(|(_, report)| report.clone()),
        "activation":"Native ammo-gated sword profiles at their accepted strike phase, including misses",
        "falloff":"One inside the source inner radius, linear to zero at the outer radius",
        "retained_native_behavior":["Attack selection, timing, cancellation and ammo costs", "Warmind recipient filters and visual presentation", "Target resistance and difficulty scaling"],
        "limits":["Push strength scales source instantaneous acceleration by the gameplay calibration that gives the Concussion Rifle the strength of Chosen of the Warmind's stronger push. Physical equivalence is not established.",
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
    if let Some((table, _)) = surface {
        result["surface"] = json!("sword-surface");
        result["source_surface"] = json!(table);
    }
    // The hammer strikes where the player stands. Every trajectory block and the `+204` distance
    // move zero metres, as the stock relic slam rows store them. Its airborne strike is a downward
    // smash, which a stock sword's powered aerial row selects with action 7843564F in place of
    // the air lunge 054A16BE. That row keeps no forward lane at `+1B4`. The fall itself comes
    // from the action, since its human clip has no root motion.
    result["lunge"] = json!(false);
    result["aerial_action"] = json!({"replaces":AIR_LUNGE,"action":AIR_SLAM});
    Ok(Some(result))
}
