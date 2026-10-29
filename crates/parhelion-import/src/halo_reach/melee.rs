//! Bind translated strikes to private weapon melee activation and source swing audio.
use super::*;
use crate::tiger::reader::Reader;
pub(super) mod slam;

pub(super) fn prepare(
    native: &mut Reader,
    behavior: &Value,
    graph: &mut Value,
    recipe: &mut Value,
    root: &Path,
    source_audio: bool,
) -> Result<Value> {
    let strikes = graph["animation"]["first_person"]["melee"]["strikes"]
        .as_array()
        .context("Translated melee strikes are absent")?
        .clone();
    ensure!(
        !strikes.is_empty(),
        "Translated melee strike corpus is empty"
    );
    if !source_audio {
        ensure!(
            graph["audio"].is_null(),
            "Unconfigured melee audio already has a route"
        );
        let runtime = u32::try_from(
            graph["animation"]["runtime_entity"]
                .as_u64()
                .context("Melee runtime entity")?,
        )?;
        graph["audio"] = json!({"authoring_schema":3,"runtime_entity":format!("{runtime:08X}"),
            "transcoded_media":[],"routing":"native_melee_contacts"});
    }
    let audio = &mut graph["audio"];
    ensure!(
        audio["authoring_schema"] == 3 && audio.get("impact_key").is_none(),
        "Weapon melee requires unambiguous source audio routing: {audio}"
    );
    let bus = if source_audio {
        Some(u32::try_from(
            audio["output_bus"]
                .as_u64()
                .context("Melee source audio bus")?,
        )?)
    } else {
        None
    };
    let recordings = root
        .parent()
        .context("Melee import root")?
        .join("source/audio");
    let mut converted = BTreeMap::<u64, Value>::new();
    let mut events = match audio.get("clip_events") {
        None => Vec::new(),
        Some(rows) => rows.as_array().context("Clip event rows")?.clone(),
    };
    for strike in &strikes {
        let Some(bus) = bus else { continue };
        ensure!(
            strike["source_audio"] == true,
            "Melee sound timing differs from its configured source audio"
        );
        let source_id = strike["source_sound"]["datum"]
            .as_u64()
            .context("Strike source sound")?;
        let source = if let Some(source) = converted.get(&source_id) {
            source.clone()
        } else {
            let source = &behavior["nodes"][format!("{source_id:08X}")];
            ensure!(
                source["tag"] == strike["source_sound"],
                "Strike audio source differs from its animation table"
            );
            let source = super::audio::clips::cue(source, &recordings, root, bus, audio)?;
            converted.insert(source_id, source.clone());
            source
        };
        let native_sound = u32::from_str_radix(
            strike["native_sound"]
                .as_str()
                .context("Strike native sound")?,
            16,
        )?;
        let native_sound = crate::tiger::audio::sound_assets(native, native_sound, false)?;
        let row = json!({"clip_name":strike["clip_name"],"event_index":strike["sound_event_index"],
            "sounds":[source],"native_sounds":[native_sound],"source_clip":strike["source_clip"]});
        ensure!(
            events.iter().all(|old| {
                old["clip_name"] != row["clip_name"] || old["event_index"] != row["event_index"]
            }),
            "Strike cue repeats an existing clip event"
        );
        events.push(row);
    }
    let namespace = recipe["namespace"]
        .as_str()
        .context("Melee private namespace")?;
    let key = format!("{namespace}.contact-feedback")
        .bytes()
        .fold(0x811C9DC5u32, |key, byte| {
            key.wrapping_mul(16777619) ^ u32::from(byte)
        });
    ensure!(
        ![0, u32::MAX].contains(&key),
        "Invalid private melee identity"
    );
    audio["clip_events"] = json!(events);
    audio["impact_key"] = json!(format!("{key:08X}"));
    audio["retain_native_contacts"] = json!(true);
    let activation =
        crate::d2_mot::glaive::weapon_melee(native, recipe, key, "Reach Weapon Melee")?;
    let root_id = behavior["root"]["datum"].clone();
    let source_damage = behavior["edges"]
        .as_array()
        .context("Melee source edges")?
        .iter()
        .filter(|edge| edge["owner"] == root_id && edge["role"] == "melee_damage")
        .map(|edge| edge["tag"].clone())
        .collect::<Vec<_>>();
    ensure!(
        source_damage.len() == 1,
        "Source weapon needs one melee damage profile"
    );
    let damage_tag = source_damage[0]["datum"]
        .as_u64()
        .context("Source melee damage tag")?;
    let damage = &behavior["nodes"][format!("{damage_tag:08X}")]["data"];
    let amount = damage["lower_bound"]
        .as_f64()
        .context("Source melee damage amount")? as f32;
    let upper = damage["upper_bound"]
        .as_array()
        .filter(|bounds| bounds.len() == 2)
        .context("Source melee damage bounds")?;
    ensure!(
        amount.is_finite()
            && amount > 0.
            && upper
                .iter()
                .all(|value| value.as_f64().is_some_and(|value| value as f32 == amount))
            && damage["radius"] == json!([0., 0.])
            && damage["core_radius"] == 0.0,
        "Source melee damage requires a variable or radial native profile"
    );
    graph["audio"]["contact_damage"] = json!(amount);
    Ok(
        json!({"activation":activation,"strikes":strikes.len(),"source_sounds":converted.len(),"source_damage":source_damage,
        "authored_contact_damage":amount,"damage_policy":"Source constant amount in a private absolute native profile",
        "swing_audio":if source_audio {"source_recording"} else {"native_punch_at_contact"},
        "retained_native_behavior":["Native surface feedback", "Native lunge, hit acceptance and attack input handling", "Post-melee weapon ready transition"],
        "limits":["Source strike motion, primary keyframe and constant contact damage are translated. Source swing recordings require configured audio banks and a decoder. Target resistance, power and difficulty multipliers remain native. Melee clash, source hit effects, sound preemption fades and environment routing are not translated. The private attack requires repeated hit, miss, selection and subclass-bank gameplay checks."]}),
    )
}
