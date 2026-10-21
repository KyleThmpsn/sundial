//! Named attachment events keep native trigger and acoustic profiles with source bank behavior.
use super::*;

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("audio hexadecimal identity")?,
        16,
    )?)
}

fn word(value: &Value) -> Result<u32> {
    Ok(u32::try_from(
        value.as_u64().context("source audio identity")?,
    )?)
}

fn sound(source: &Source, tag: u32, path: &str) -> Result<Value> {
    let cue = Cue::read(source, tag)?;
    ensure!(
        cue.media.iter().all(|tag| *tag != u32::MAX),
        "named audio has empty media slots"
    );
    let media = cue
        .media
        .iter()
        .map(|tag| format!("{tag:08X}"))
        .collect::<Vec<_>>();
    let ids = cue
        .media
        .iter()
        .map(|tag| source.reference(*tag).map(|id| format!("{id:08X}")))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"tag":format!("{tag:08X}"),"event_id":format!("{:08X}",cue.events[0]),
        "secondary_event_id":format!("{:08X}",cue.events[1]),"bank":format!("{:08X}",cue.bank),
        "settings":cue.settings,"media":media,"media_ids":ids,"event_path":path}),
    )
}

fn group(source: &Source, owner: u32, parent: u32) -> Result<Value> {
    let p = source.read(parent)?;
    // Source parents use generated declaration classes. The fixed parent envelope and
    // the pointed class establish this group, rather than any one generated handle.
    ensure!(
        p.0.len() == 24 && p.u64(0)? == 24 && p.u64(8)? == 0 && p.u32(20)? == 0,
        "source named audio parent envelope differs"
    );
    let tag = p.u32(16)?;
    ensure!(
        source.reference(tag)? == 0x80800C96,
        "source named group class differs"
    );
    let p = source.read(tag)?;
    let mut events = Vec::new();
    let mut hashes = BTreeSet::new();
    for row in p.array(8, 24, Some(0x808003FD))? {
        let hash = p.u32(row)?;
        ensure!(
            hashes.insert(hash) && p.u32(row + 4)? == 0,
            "source named event identity differs"
        );
        let mut sounds = Vec::new();
        for at in p.array(row + 8, 32, Some(0x8080060E))? {
            ensure!(
                p.u32(at)? == 0x00030000
                    && p.u32(at + 4)? == 0
                    && p.u32(at + 8)? == 0x811C9DC5
                    && p.u32(at + 12)? == 0
                    && p.u32(at + 28)? == 0,
                "source named event has unsupported dispatch fields"
            );
            let path = p.pointer(at + 16)?;
            let bytes = p.0.get(path..).context("source named event path")?;
            let end = bytes
                .iter()
                .position(|b| *b == 0)
                .context("unterminated named event path")?;
            let path = std::str::from_utf8(&bytes[..end])?;
            ensure!(
                (path.starts_with("content/") || path.starts_with("content\\"))
                    && path.ends_with(".wwise_event"),
                "named event requires sequence translation"
            );
            sounds.push(sound(source, p.u32(at + 24)?, path)?);
        }
        if !sounds.is_empty() {
            events.push(json!({"hash":format!("{hash:08X}"),"sounds":sounds}));
        }
    }
    Ok(
        json!({"owner":format!("{owner:08X}"),"parent":format!("{parent:08X}"),
        "group":format!("{tag:08X}"),"events":events}),
    )
}

fn inspect(source: &Source, runtime: &Value) -> Result<Value> {
    let content = word(&runtime["content_key"])?;
    let mut groups = Vec::new();
    for component in runtime["components"]
        .as_array()
        .context("source runtime components")?
    {
        if word(&component["class"])? != 0x80802580 {
            continue;
        }
        let tag = word(&component["owner"])?;
        ensure!(
            source.reference(tag)? == 0x80800861,
            "source attachment owner class differs"
        );
        let p = source.read(tag)?;
        let r = p.pointer(24)?;
        ensure!(
            p.u32(r - 4)? == 0x80802580,
            "source attachment resource differs"
        );
        let selected = p
            .array(r + 0x5D8, 0xD8, Some(0x808031DE))?
            .into_iter()
            .filter(|at| p.u32(*at).ok() == Some(content))
            .collect::<Vec<_>>();
        ensure!(
            selected.len() <= 1,
            "source named audio selection is ambiguous"
        );
        let row = selected.first().copied().unwrap_or(r + 0x500);
        let parent = p.u32(row + 0x88)?;
        if present(parent) {
            groups.push(group(source, tag, parent)?);
        }
    }
    Ok(json!({"named_groups":groups,"content_key":content}))
}

pub(in crate::d1_roi) fn prepare(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    target: &mut Value,
    folder: &Path,
) -> Result<Value> {
    let source_routing = inspect(source, runtime)?;
    crate::tiger::audio::add_to_rig(native, target, false)?;
    let mut report = crate::tiger::audio::compatibility(&source_routing, &target["audio"]);
    report["source_routing"] = source_routing;
    let events = report["matched_events"]
        .as_array()
        .context("matched named audio")?;
    if events.is_empty() {
        return Ok(report);
    }
    let mut cues = BTreeSet::new();
    let mut settings = BTreeMap::new();
    for event in events {
        let sources = event["sounds"].as_array().context("named source sounds")?;
        let targets = event["native_sounds"]
            .as_array()
            .context("named native sounds")?;
        // A single cue per event keeps trigger multiplicity and avoids pairing distinct
        // layered cue lists by incidental order. Layers within its bank remain intact.
        ensure!(
            sources.len() == 1 && targets.len() == 1,
            "named audio cue multiplicity needs translation"
        );
        let cue = hex(&sources[0]["tag"])?;
        let original = Cue::read(source, cue)?;
        let native_cue = native.tag(hex(&targets[0]["tag"])?, Some(0x80809802))?;
        let profile = native_cue.u32(0x10)?;
        if let Some(previous) = settings.insert(original.settings, profile) {
            ensure!(
                previous == profile,
                "source cue has conflicting native acoustic profiles"
            );
        }
        cues.insert(cue);
    }
    let directory = folder.join("named-audio");
    let prepared = prepare_with_settings(source, native, &directory, &cues, Some(&settings))?;
    let owned = prepared["nodes"]
        .as_array()
        .context("prepared audio nodes")?
        .iter()
        .filter_map(|node| node.get("audio_bank"))
        .map(|bank| serde_json::from_value::<bank::Bank>(bank.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .flat_map(|bank| bank.objects)
        .collect::<BTreeSet<_>>();
    let mut media = Vec::new();
    let mut banks = Vec::new();
    for node in prepared["nodes"]
        .as_array()
        .context("prepared audio nodes")?
    {
        if node["storage"] == "audio_media" {
            media.push(json!({"source_tag":format!("{:08X}",word(&node["source_media_tag"])?),
                "media_id":format!("{:08X}",word(&node["source_media_id"])?),
                "file":format!("named-audio/{}",node["file"].as_str().context("audio file")?),"codec":"pcm_s16le"}));
        } else if node["storage"] == "audio_bank" {
            let bank: bank::Bank = serde_json::from_value(node["audio_bank"].clone())?;
            ensure!(
                bank.bank_dependencies().is_empty(),
                "named cue depends on another bank"
            );
            ensure!(
                bank.external_objects().is_disjoint(&owned),
                "named cue depends on private objects in another bank"
            );
            let tag = word(&node["source_bank_tag"])?;
            let file = format!("named-audio/{tag:08X}.bank.json");
            fs::write(folder.join(&file), serde_json::to_vec(&bank)?)?;
            banks.push(json!({"source_tag":format!("{tag:08X}"),"file":file,"version":113,"routing":"source"}));
        }
    }
    report["authoring_schema"] = json!(3);
    report["runtime_entity"] = target["runtime_entity"].clone();
    report["transcoded_media"] = json!(media);
    report["converted_banks"] = json!(banks);
    report["bank_fallbacks"] = json!([]);
    report["cue_settings"] = json!("selected_native_event");
    report["source_media_imported"] = json!(true);
    Ok(report)
}
