//! A private launch sound follows an accepted native projectile spawn.
use super::super::secondary::Bundle;
use super::*;
use crate::{
    halo_reach::{audio, cache::Cache},
    tiger::audio::bank::Bank,
};

fn primary<'a>(behavior: &'a Value, cache: &Cache) -> Result<Option<&'a Value>> {
    let edges = behavior["edges"]
        .as_array()
        .context("Vehicle weapon edges")?;
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Vehicle source identity")?;
    let Some(weapon) = edges
        .iter()
        .filter(|e| e["owner"] == root && e["role"] == "vehicle_weapon")
        .min_by_key(|e| e["offset"].as_u64())
    else {
        return Ok(None);
    };
    let id = weapon["tag"]["datum"]
        .as_u64()
        .context("Vehicle weapon identity")?;
    let node = &behavior["nodes"][format!("{id:08X}")];
    let triggers = node["data"]["object"]["gameplay"]["triggers"]
        .as_array()
        .context("Vehicle weapon triggers")?;
    let primary = triggers
        .iter()
        .filter(|t| t["input"] == 0)
        .collect::<Vec<_>>();
    ensure!(
        primary.len() == 1 && primary[0]["secondary_barrel"] == -1,
        "Vehicle primary input needs a unique single barrel"
    );
    let index = usize::try_from(
        primary[0]["primary_barrel"]
            .as_u64()
            .context("Primary barrel index")?,
    )?;
    let at = usize::try_from(
        node["tag"]["offset"]
            .as_u64()
            .context("Vehicle weapon source address")?,
    )?;
    let barrels = cache.block(at + 0x408, 0x184)?;
    let barrel = *barrels
        .get(index)
        .context("Primary barrel is outside source weapon")?;
    let effects = cache.block(barrel + 0x178, 0xc4)?;
    let roots = edges
        .iter()
        .filter(|e| {
            e["owner"] == id
                && e["role"] == "fire"
                && e["offset"]
                    .as_u64()
                    .is_some_and(|offset| effects.iter().any(|row| offset == (*row + 4) as u64))
        })
        .map(|e| e["tag"]["datum"].as_u64().context("Primary firing effect"))
        .collect::<Result<Vec<_>>>()?;
    audio::fire_sound_from(behavior, roots)
}

fn cue(
    native: &mut Reader,
    graph: &mut Graph,
    sound: &Value,
    template: u32,
    bus: u32,
) -> Result<Value> {
    let original = native.tag(template, Some(0x80809802))?;
    let bank_tag = original.u32(20)?;
    audio::verify_bus(&native.tag(bank_tag, None)?.0, bus)?;
    let source = &sound["data"];
    ensure!(
        source["promotion_index"] == -1 && source["custom_playback_index"] == -1,
        "Vehicle firing sound requires source promotion or custom playback"
    );
    let ranges = source["pitch_ranges"]
        .as_array()
        .context("Vehicle firing pitch ranges")?;
    ensure!(
        ranges.len() == 1,
        "Vehicle firing sound requires range switching"
    );
    let variants = ranges[0]["variants"]
        .as_array()
        .context("Vehicle firing variations")?;
    ensure!(
        !variants.is_empty(),
        "Vehicle firing sound has no variations"
    );
    let limits = &source["playback_limits"];
    ensure!(
        limits["preemption_ms"] == 0 && limits["maximum_per_object_per_class"] == 0,
        "Vehicle firing sound requires timed or class preemption"
    );
    let limit = u16::try_from(
        limits["maximum_per_tag"]
            .as_u64()
            .filter(|v| (1..=16).contains(v))
            .context("Vehicle firing global voice limit")?,
    )?;
    let gain = source["playback"]["gain_db"]
        .as_f64()
        .context("Vehicle firing gain")? as f32;
    let source_root = graph
        .root
        .parent()
        .context("Vehicle source directory")?
        .join("source/audio");
    let medium = original.u32(
        *original
            .array(0x18, 4, Some(0x80800014))?
            .first()
            .context("Native firing medium")?,
    )?;
    let mut ids = Vec::new();
    let mut media = Vec::new();
    for variant in variants {
        let id = u32::try_from(
            variant["fsb_info"]
                .as_u64()
                .context("Vehicle firing sample")?,
        )?;
        ensure!(
            !ids.contains(&id),
            "Vehicle firing variations repeat a sample"
        );
        ids.push(id);
        let symbol = format!("vehicle-fire-media-{id:08X}");
        let data = crate::tiger::audio::transcode::normalize_pcm_wem(&std::fs::read(
            source_root.join(format!("{id:08X}.wav")),
        )?)?;
        graph.add(&symbol, medium, &data, None, Vec::new())?;
        graph.nodes.last_mut().unwrap()["storage"] = json!("audio_media");
        media.push(json!({"id":id,"symbol":symbol}));
    }
    let bank = Bank::one_shot_global(&ids, bus, gain, limit)?;
    graph.add("vehicle-fire-bank", bank_tag, &bank.bytes, None, Vec::new())?;
    let node = graph.nodes.last_mut().unwrap();
    node["storage"] = json!("audio_bank");
    node["audio_bank"] = serde_json::to_value(&bank)?;
    node["audio_media"] = json!(media);
    ensure!(
        original.0.len() >= 0x50 && original.pointer(0x20)? == 0x40,
        "Vehicle firing cue envelope differs"
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
    let mut patches = vec![json!({"offset":20,"symbol":"vehicle-fire-bank"})];
    for (index, id) in ids.iter().enumerate() {
        patches
            .push(json!({"offset":0x50+index*4,"symbol":format!("vehicle-fire-media-{id:08X}")}));
    }
    graph.add("vehicle-fire-cue", template, &data, None, patches)?;
    graph.nodes.last_mut().unwrap()["audio_events"] = json!([{"offset":8,"id":1}]);
    Ok(
        json!({"source":sound["tag"],"variations":ids,"gain_db":gain,"global_voice_limit":limit,"output_bus":bus}),
    )
}

impl Assembly {
    pub fn fire_audio(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        behavior: &Value,
        cache: &Cache,
        bus: u32,
    ) -> Result<Value> {
        let Some(sound) = primary(behavior, cache)? else {
            return Ok(json!({"status":"source_only","reason":"No source primary firing sound"}));
        };
        let mut barrels = Vec::new();
        for (&owner, payload) in &self.payloads {
            for definition in (0..payload.0.len().saturating_sub(0xb94)).step_by(8) {
                if payload.u32(definition)? != owner || payload.u32(definition + 4)? != 0x80803889 {
                    continue;
                }
                let instance = usize::try_from(payload.u64(definition + 8)?)?;
                if instance + 16 > payload.0.len()
                    || payload.u32(instance)? != owner
                    || payload.u32(instance + 4)? != 0x80803865
                    || payload.u64(instance + 8)? != definition as u64
                {
                    continue;
                }
                let tag = payload.u32(definition + 0xb80)?;
                ensure!(
                    tag == payload.u32(definition + 0xb90)? && ![0, u32::MAX].contains(&tag),
                    "Vehicle barrel has different native projectile routes"
                );
                barrels.push((owner, definition, tag));
            }
        }
        ensure!(
            barrels.len() == 1,
            "Vehicle primary native barrel is absent or ambiguous"
        );
        let (owner, definition, projectile) = barrels[0];
        ensure!(
            native.tag(projectile, Some(0x80809c0f))?.u16(0x96)? == 18,
            "Vehicle firing route is not a projectile"
        );
        // This native once-flow has no input connections. Its complete registration
        // and instance allocation are added without changing projectile behavior.
        let host = 0x80fc7616;
        let template = native.tag(host, Some(0x80809c0f))?;
        let mut candidates = Vec::new();
        for row in template.array(16, 12, Some(0x80809c04))? {
            let tag = template.u32(row)?;
            let p = native.tag(tag, Some(0x80809c36))?;
            let d = usize::try_from(p.u64(p.pointer(16)? + 8)?)?;
            if p.u32(d - 4)? != 0x808084e9 {
                continue;
            }
            ensure!(
                p.array(d + 0x188, 40, Some(0x80809789))?.is_empty(),
                "Native launch sound needs inputs"
            );
            let events = p.array(d + 0x168, 24, Some(0x808093e6))?;
            ensure!(
                events.len() == 1,
                "Native launch sound flow is not a single event"
            );
            let event = p.pointer(events[0] + 16)?;
            ensure!(
                p.u32(event - 4)? == 0x80806b38,
                "Native launch event is not a once sound"
            );
            candidates.push((tag, event + 0x40, p.u32(event + 0x40)?));
        }
        ensure!(
            candidates.len() == 1,
            "Native launch sound template is absent or ambiguous"
        );
        let (flow, field, source_cue) = candidates[0];
        let report = cue(native, graph, sound, source_cue, bus)?;
        let mut bundle = Bundle::read(native, projectile)?;
        bundle.add_unconnected(native, flow, host)?;
        bundle
            .resources
            .entry(flow)
            .or_default()
            .insert(field, "vehicle-fire-cue".into());
        let symbol = bundle.emit(graph, "vehicle-fire-projectile")?;
        for offset in [0xb80, 0xb90] {
            self.resource(owner, definition + offset, symbol.clone())?;
        }
        Ok(
            json!({"sound":report,"projectile":symbol,"native_projectile":format!("{projectile:08X}"),
            "trigger":"Native accepted projectile spawn","gameplay_verified":false,
            "limits":["Native projectile gameplay and visuals are retained. Source primary firing PCM uses a private global voice limit. Source pitch curves, spatial attenuation and secondary firing are not translated. Native activation and audio cleanup require gameplay acceptance."]}),
        )
    }
}
