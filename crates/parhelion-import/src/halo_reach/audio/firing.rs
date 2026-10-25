//! Simultaneous source firing recordings on a private native event.
use super::*;
use crate::tiger::audio::bank::{Bank, ShotLayer};

pub(super) fn validate(behavior: &Value, roots: &[u64], sounds: &[&Value]) -> Result<()> {
    if sounds.len() <= 1 {
        return Ok(());
    }
    let [root] = roots else {
        anyhow::bail!("Layered firing requires one source effect");
    };
    let effect = &behavior["nodes"][format!("{root:08X}")];
    ensure!(
        effect["tag"]["group"] == "effe" && effect["data"]["loop_start"] == -1,
        "Layered firing requires a finite source effect"
    );
    let events = effect["data"]["events"]
        .as_array()
        .context("Layered firing events")?;
    let event = events.first().context("Layered firing event")?;
    ensure!(
        event["flags"] == 0
            && event["skip_fraction"] == 0.0
            && event["delay_seconds"] == json!([0., 0.]),
        "Layered firing requires an immediate unconditional event"
    );
    let wanted = sounds
        .iter()
        .map(|sound| {
            sound["tag"]["datum"]
                .as_u64()
                .context("Firing sound identity")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        events
            .iter()
            .skip(1)
            .all(|event| event["parts"]
                .as_array()
                .is_some_and(|parts| parts.iter().all(|part| part["tag"]["datum"]
                    .as_u64()
                    .is_none_or(|id| !wanted.contains(&id))))),
        "Firing layers repeat in later source events"
    );
    let mut found = BTreeSet::new();
    for part in event["parts"].as_array().context("Firing effect parts")? {
        let Some(id) = part["tag"]["datum"]
            .as_u64()
            .filter(|id| wanted.contains(id))
        else {
            continue;
        };
        ensure!(
            part["environment"] == 0
                && part["violence"] == 0
                && part["flags"] == 0
                && part["camera"] == 0
                && found.insert(id),
            "Layered firing requires unique unconditional source parts"
        );
    }
    ensure!(
        found == wanted,
        "Firing layers have different source event times"
    );
    Ok(())
}

fn layer(
    sound: &Value,
    source: &Path,
    graph: &Path,
    files: &mut BTreeMap<u32, Value>,
) -> Result<(ShotLayer, Value)> {
    let ranges = sound["data"]["pitch_ranges"]
        .as_array()
        .context("Sound ranges")?;
    let [range] = ranges.as_slice() else {
        anyhow::bail!("Primary firing sound requires pitch-range switching");
    };
    let variants = range["variants"].as_array().context("Sound variations")?;
    ensure!(
        !variants.is_empty(),
        "Primary firing sound has no variations"
    );
    let mut media = Vec::new();
    for variant in variants {
        let id = u32::try_from(variant["fsb_info"].as_u64().context("Sound identity")?)?;
        let name = format!("{id:08X}");
        let file = format!("audio/{name}.wem");
        if let std::collections::btree_map::Entry::Vacant(entry) = files.entry(id) {
            let pcm = crate::tiger::audio::transcode::normalize_pcm_wem(&fs::read(
                source.join(format!("{name}.wav")),
            )?)?;
            fs::write(graph.join(&file), pcm)?;
            entry.insert(json!({"source_tag":name,"file":file}));
        }
        media.push(id);
    }
    let gain_db = sound["data"]["playback"]["gain_db"]
        .as_f64()
        .context("Source firing gain")? as f32;
    let limits = &sound["data"]["playback_limits"];
    ensure!(
        sound["data"]["promotion_index"] == -1 && sound["data"]["custom_playback_index"] == -1,
        "Primary firing sound requires promotion or custom playback translation"
    );
    ensure!(
        limits["preemption_ms"] == 0 && limits["maximum_per_object_per_class"] == 0,
        "Primary firing sound requires timed preemption or shared class limits"
    );
    let voice_limit = limits["maximum_per_object_per_tag"]
        .as_u64()
        .filter(|v| (1..=16).contains(v))
        .context("Invalid source per-object firing voice limit")? as u16;
    let report = json!({"source":sound["tag"],"source_playback":sound["data"]["playback"],
        "source_playback_limits":limits,"native_voice_limit":voice_limit,
        "media":media.iter().map(|id|format!("{id:08X}")).collect::<Vec<_>>()});
    Ok((
        ShotLayer {
            media,
            gain_db,
            voice_limit,
        },
        report,
    ))
}

pub(super) fn convert(
    sounds: &[&Value],
    source: &Path,
    graph: &Path,
    output_bus: u32,
) -> Result<(Value, Vec<Value>)> {
    fs::create_dir_all(graph.join("audio"))?;
    let mut files = BTreeMap::new();
    let mut layers = Vec::new();
    let mut reports = Vec::new();
    for sound in sounds {
        let (layer, report) = layer(sound, source, graph, &mut files)?;
        layers.push(layer);
        reports.push(report);
    }
    let bank = Bank::one_shot_layers(&layers, output_bus)?;
    fs::write(
        graph.join("audio/fire.bank.json"),
        serde_json::to_vec(&bank)?,
    )?;
    let mut report = reports.first().context("Empty firing layers")?.clone();
    let media = files
        .keys()
        .map(|id| format!("{id:08X}"))
        .collect::<Vec<_>>();
    report["media"] = json!(media);
    report["media_ids"] = json!(media);
    report["bank"] = json!("00000001");
    report["event_id"] = json!("00000001");
    report["routing"] = json!("source_one_shot");
    report["layers"] = json!(reports);
    Ok((report, files.into_values().collect()))
}
