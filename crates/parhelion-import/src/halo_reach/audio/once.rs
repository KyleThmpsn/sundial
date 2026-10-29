//! Source one-shot PCM on a private cue with a shared transient-object voice limit.
use crate::{
    presentation::{Graph, put},
    tiger::{audio::bank::Bank, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(in crate::halo_reach) fn cue(
    native: &mut Reader,
    graph: &mut Graph,
    sound: &Value,
    template: u32,
    bus: Option<u32>,
    prefix: &str,
) -> Result<Value> {
    ensure!(
        !prefix.is_empty()
            && prefix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
        "Invalid one-shot graph namespace"
    );
    let bank_symbol = format!("{prefix}-bank");
    let cue_symbol = format!("{prefix}-cue");
    let original = native.tag(template, Some(0x80809802))?;
    let bank_tag = original.u32(20)?;
    let bank = native.tag(bank_tag, None)?;
    let bus = super::output_bus(&bank.0, bus, original.u32(8)?)?;
    super::verify_bus(&bank.0, bus)?;
    let source = &sound["data"];
    ensure!(
        source["promotion_index"] == -1 && source["custom_playback_index"] == -1,
        "Source one-shot sound requires source promotion or custom playback"
    );
    let ranges = source["pitch_ranges"]
        .as_array()
        .context("Source one-shot pitch ranges")?;
    ensure!(
        ranges.len() == 1,
        "Source one-shot sound requires range switching"
    );
    let variants = ranges[0]["variants"]
        .as_array()
        .context("Source one-shot variations")?;
    ensure!(
        !variants.is_empty(),
        "Source one-shot sound has no variations"
    );
    let limits = &source["playback_limits"];
    ensure!(
        limits["preemption_ms"] == 0 && limits["maximum_per_object_per_class"] == 0,
        "Source one-shot sound requires timed or class preemption"
    );
    let limit = u16::try_from(
        limits["maximum_per_tag"]
            .as_u64()
            .filter(|v| (1..=16).contains(v))
            .context("Source one-shot global voice limit")?,
    )?;
    let gain = source["playback"]["gain_db"]
        .as_f64()
        .context("Source one-shot gain")? as f32;
    let source_root = graph
        .root
        .parent()
        .context("One-shot source directory")?
        .join("source/audio");
    let medium = original.u32(
        *original
            .array(0x18, 4, Some(0x80800014))?
            .first()
            .context("Native one-shot medium")?,
    )?;
    let mut ids = Vec::new();
    let mut media = Vec::new();
    for variant in variants {
        let id = u32::try_from(
            variant["fsb_info"]
                .as_u64()
                .context("Source one-shot sample")?,
        )?;
        ensure!(
            !ids.contains(&id),
            "Source one-shot variations repeat a sample"
        );
        ids.push(id);
        let symbol = format!("{prefix}-media-{id:08X}");
        let data = crate::tiger::audio::transcode::normalize_pcm_wem(&std::fs::read(
            source_root.join(format!("{id:08X}.wav")),
        )?)?;
        graph.add(&symbol, medium, &data, None, Vec::new())?;
        graph.nodes.last_mut().unwrap()["storage"] = json!("audio_media");
        media.push(json!({"id":id,"symbol":symbol}));
    }
    let bank = Bank::one_shot_global(&ids, bus, gain, limit)?;
    graph.add(&bank_symbol, bank_tag, &bank.bytes, None, Vec::new())?;
    let node = graph.nodes.last_mut().unwrap();
    node["storage"] = json!("audio_bank");
    node["audio_bank"] = serde_json::to_value(&bank)?;
    node["audio_media"] = json!(media);
    ensure!(
        original.0.len() >= 0x50 && original.pointer(0x20)? == 0x40,
        "Source one-shot cue envelope differs"
    );
    let mut data = original.0[..0x50].to_vec();
    data.resize(0x50 + ids.len() * 4, 0xff);
    let size = data.len() as u64;
    put(&mut data, 0, &size.to_le_bytes())?;
    put(&mut data, 8, &1u32.to_le_bytes())?;
    put(&mut data, 12, &u32::MAX.to_le_bytes())?;
    put(&mut data, 20, &u32::MAX.to_le_bytes())?;
    for at in [0x18, 0x40] {
        put(&mut data, at, &(ids.len() as u64).to_le_bytes())?;
    }
    let mut patches = vec![json!({"offset":20,"symbol":bank_symbol})];
    for (index, id) in ids.iter().enumerate() {
        patches.push(json!({"offset":0x50+index*4,"symbol":format!("{prefix}-media-{id:08X}")}));
    }
    graph.add(&cue_symbol, template, &data, None, patches)?;
    graph.nodes.last_mut().unwrap()["audio_events"] = json!([{"offset":8,"id":1}]);
    Ok(
        json!({"source":sound["tag"],"variations":ids,"gain_db":gain,"global_voice_limit":limit,"output_bus":bus,"source_playback":source["playback"],"source_playback_limits":limits,"limits":["Nonspatial playback at fixed pitch retains source recordings and base gain. Source attenuation, environment tails, pitch modulation, skip fractions, randomized gain and per-object limits are not reproduced."]}),
    )
}
