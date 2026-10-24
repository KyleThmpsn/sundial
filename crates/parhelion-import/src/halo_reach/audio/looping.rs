//! Explicit attached source tracks on an existing native firing loop.
use super::*;
use crate::tiger::audio::bank::{
    Bank,
    looping::{Layer, Phase},
};

fn scalar(value: &Value, label: &str) -> Result<f32> {
    let value = value
        .as_f64()
        .with_context(|| format!("Firing loop {label}"))? as f32;
    ensure!(value.is_finite(), "Nonfinite firing loop {label}");
    Ok(value)
}

fn fade(value: &Value) -> Result<u32> {
    let seconds = scalar(&value["seconds"], "fade")?;
    ensure!(
        value["mode"] == 0 && (0.0..=60.0).contains(&seconds),
        "Firing loop requires another fade mode"
    );
    Ok((seconds * 1000.).round() as u32)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn native(
    reader: &mut crate::tiger::reader::Reader,
    rig: &Value,
    behavior: &Value,
    source: &Path,
    graph: &Path,
    options: &Options,
    path: &str,
    slot: u8,
    node: u32,
) -> Result<Value> {
    ensure!(slot < 6, "Invalid native firing loop slot");
    let nodes = behavior["nodes"]
        .as_object()
        .context("Firing loop source nodes")?;
    let selected = nodes
        .values()
        .filter(|n| n["tag"]["group"] == "lsnd" && n["tag"]["path"] == path.replace('/', "\\"))
        .collect::<Vec<_>>();
    ensure!(
        selected.len() == 1,
        "Source firing loop is absent or ambiguous"
    );
    let source_loop = selected[0];
    ensure!(
        behavior["edges"]
            .as_array()
            .context("Source loop edges")?
            .iter()
            .any(|e| e["owner"] == behavior["root"]["datum"]
                && e["role"] == "attachment"
                && e["tag"]["datum"] == source_loop["tag"]["datum"]),
        "Selected firing loop is not attached to the source weapon"
    );
    let candidates = rig["audio"]["unnamed_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| {
            g["sounds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |s| (g, s))
        })
        .filter(|(_, s)| {
            s["root_slot"] == slot
                && s["node_name"] == format!("{node:08X}")
                && s["component_class"] == "808084E9"
                && s["node_kind"] == "loop"
        })
        .collect::<Vec<_>>();
    ensure!(
        candidates.len() == 1,
        "Native firing loop is absent or ambiguous"
    );
    let (presentation, native) = candidates[0];
    let stop = u32::from_str_radix(
        native["secondary_event_id"]
            .as_str()
            .context("Native loop stop event")?,
        16,
    )?;
    ensure!(
        !matches!(stop, 0 | u32::MAX | 0x811c9dc5),
        "Native loop has no stop event"
    );
    let output_bus = selection::output_bus(
        &reader
            .tag(
                u32::from_str_radix(native["bank"].as_str().context("Native loop bank")?, 16)?,
                None,
            )?
            .0,
        options.fire_bus,
        u32::from_str_radix(
            native["event_id"]
                .as_str()
                .context("Native loop start event")?,
            16,
        )?,
    )?;
    let mut layers = Vec::new();
    let mut ids = BTreeSet::<u32>::new();
    let tracks = source_loop["data"]["track_controls"]
        .as_array()
        .context("Source loop tracks")?;
    ensure!(!tracks.is_empty(), "Source firing loop has no tracks");
    for track in tracks {
        ensure!(
            track["output_effect"] == 0
                && track["layer"] == 0
                && track["layer_flags"] == 0
                && [
                    "alternate_loop",
                    "alternate_end",
                    "transition_in",
                    "transition_out"
                ]
                .iter()
                .all(|&key| track[key].is_null()),
            "Firing loop requires alternate tracks, layers or effects"
        );
        ensure!(
            !track["loop"].is_null(),
            "Firing track has no sustained recording"
        );
        for (key, phase) in [
            ("start", Phase::Start),
            ("loop", Phase::Sustain),
            ("end", Phase::Stop),
        ] {
            if track[key].is_null() {
                continue;
            }
            let tag = track[key]["datum"]
                .as_u64()
                .context("Track sound identity")?;
            let sound = &nodes
                .get(&format!("{tag:08X}"))
                .context("Track sound closure")?["data"];
            let ranges = sound["pitch_ranges"]
                .as_array()
                .context("Track pitch ranges")?;
            ensure!(
                ranges.len() == 1
                    && sound["promotion_index"] == -1
                    && sound["custom_playback_index"] == -1
                    && sound["playback"]["skip_fraction"] == 0.0
                    && sound["playback"]["gain_variance_db"] == 0.0
                    && sound["playback"]["pitch_bounds"] == json!([0, 0]),
                "Firing track requires pitch, promotion or variation rules"
            );
            let variants = ranges[0]["variants"]
                .as_array()
                .context("Track variations")?;
            ensure!(
                !variants.is_empty()
                    && variants.iter().all(|v| v["skip_fraction_encoded"] == 0
                        && v["gain_encoded"] == variants[0]["gain_encoded"]),
                "Firing track requires individual variation gains or skips"
            );
            let media = variants
                .iter()
                .map(|v| {
                    Ok(u32::try_from(
                        v["fsb_info"].as_u64().context("Loop recording identity")?,
                    )?)
                })
                .collect::<Result<Vec<_>>>()?;
            ids.extend(&media);
            layers.push(Layer {
                media,
                gain_db: scalar(&track["gain_db"], "track gain")?
                    + scalar(&sound["playback"]["gain_db"], "recording gain")?
                    + scalar(&variants[0]["gain_encoded"], "variation gain")?,
                phase,
                fade_in_ms: if matches!(phase, Phase::Sustain) {
                    fade(&track["fade_in"])?
                } else {
                    0
                },
                fade_out_ms: fade(&track["fade_out"])?,
                curves: Vec::new(),
            });
        }
    }
    let bank = Bank::looping_nonspatial(&layers, output_bus)?;
    fs::create_dir_all(graph.join("audio"))?;
    let mut files = Vec::new();
    let mut media = Vec::new();
    for id in ids {
        let id = format!("{id:08X}");
        let file = format!("audio/{id}.wem");
        fs::write(
            graph.join(&file),
            crate::tiger::audio::transcode::normalize_pcm_wem(&fs::read(
                source.join(format!("{id}.wav")),
            )?)?,
        )?;
        files.push(json!({"source_tag":id,"file":file}));
        media.push(id);
    }
    fs::write(
        graph.join("audio/fire.bank.json"),
        serde_json::to_vec(&bank)?,
    )?;
    let sound = json!({"source":source_loop["tag"],"source_tracks":tracks,"media":media,
        "media_ids":media,"bank":"00000001","event_id":"00000001",
        "secondary_event_id":"00000002","routing":"source_loop"});
    Ok(
        json!({"authoring_schema":3,"runtime_entity":rig["runtime_entity"],
        "matched_events":[],"firing_events":[{"sounds":[sound],"native_sounds":[native],
        "native_presentation":presentation,"mapping":"explicit_source_loop"}],
        "routing":"source_loop","transcoded_media":files,"converted_banks":[{
            "source_tag":"00000001","file":"audio/fire.bank.json","version":113,"routing":"source_loop"}],
        "conversion_errors":[],"output_bus":output_bus,
        "limits":["The selected attached loop supplies start, sustain and release recordings, gains and linear fades. Native loop activation and cleanup are retained. This nonspatial route does not translate source environment switches, heat gating, distance attenuation or the remaining weapon and impact layers. Live held fire, release, selection changes and cleanup require gameplay verification."]}),
    )
}
