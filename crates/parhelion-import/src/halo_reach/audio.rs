//! MCC permutations resolve through their FSB info identity, never filename similarity.
use super::{
    cache::file_range,
    resource::{Pages, fingerprint},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// The selected MCC FSB and its adjacent .info file.
    pub bank: PathBuf,
    /// Explicit additional banks, such as one selected dialogue language.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_banks: Vec<PathBuf>,
    /// Preserve explicit exclusions for referenced samples absent from the selected source banks.
    /// The default rejects missing samples, including an incompletely configured bank set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub record_missing_samples: bool,
    /// Explicit native Wwise bus for nonspatial primary-fire playback.
    /// It must be a bus used by the selected native firing bank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire_bus: Option<u32>,
    /// Explicit vgmstream-cli executable. No decoder is downloaded by the importer.
    pub decoder: PathBuf,
}

struct Bank {
    path: PathBuf,
    entries: BTreeMap<u32, Value>,
}
fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .context("Short FSB word")?
            .try_into()?,
    ))
}
impl Bank {
    fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let mut file = File::open(&path)?;
        let header = file_range(&mut file, 0, 60)?;
        ensure!(
            &header[..4] == b"FSB5" && word(&header, 4)? == 1,
            "Expected FSB5 version 1"
        );
        let count = word(&header, 8)? as usize;
        let header_size = word(&header, 12)? as usize;
        let names_size = word(&header, 16)? as usize;
        let data_size = word(&header, 20)? as usize;
        ensure!(
            count > 0 && count <= 1_000_000 && header_size <= 128 * 1024 * 1024,
            "Oversized or empty FSB"
        );
        let data_at = 60u64 + header_size as u64 + names_size as u64;
        ensure!(
            data_at + data_size as u64 == file.metadata()?.len(),
            "FSB section sizes differ"
        );
        let headers = file_range(&mut file, 60, header_size)?;
        let info = fs::read(path.with_extension("fsb.info"))?;
        ensure!(
            info.len() == count * 280,
            "FSB info count differs from bank"
        );
        let mut cursor = 0;
        let mut samples = Vec::new();
        for i in 0..count {
            let mode = u64::from_le_bytes(
                headers
                    .get(cursor..cursor + 8)
                    .context("Short FSB sample")?
                    .try_into()?,
            );
            let frames = (mode >> 34) & 0x3fff_ffff;
            let offset = ((mode >> 7) & 0x07ff_ffff) << 5;
            let mut channels = [1, 2, 6, 8][((mode >> 5) & 3) as usize];
            let mut rate = *[
                4000, 8000, 11000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 96000,
            ]
            .get(((mode >> 1) & 15) as usize)
            .context("Invalid FSB rate")?;
            let mut more = mode & 1 != 0;
            let mut layers = 1;
            let mut looping = Value::Null;
            cursor += 8;
            while more {
                let chunk = word(&headers, cursor)?;
                let size = ((chunk >> 1) & 0xff_ffff) as usize;
                let kind = chunk >> 25;
                cursor += 4;
                let bytes = headers
                    .get(cursor..cursor + size)
                    .context("FSB extra data exceeds header")?;
                match kind {
                    1 => channels = u32::from(*bytes.first().context("FSB channels")?),
                    2 => rate = word(bytes, 0)?,
                    3 => {
                        looping = json!({"start":word(bytes,0)?,"end_inclusive":if size>=8 {Some(word(bytes,4)?)}else{None}})
                    }
                    14 => layers = word(bytes, 0)?,
                    _ => {}
                }
                cursor += size;
                more = chunk & 1 != 0;
            }
            channels = channels
                .checked_mul(layers)
                .context("FSB channel count overflow")?;
            ensure!(
                (1..=32).contains(&channels) && (1..=768_000).contains(&rate),
                "Invalid FSB sample format"
            );
            let record = &info[i * 280..(i + 1) * 280];
            let end = record[24..]
                .iter()
                .position(|b| *b == 0)
                .context("Unterminated FSB source name")?;
            let name = std::str::from_utf8(&record[24..24 + end])?;
            let id = word(record, 0)?;
            ensure!(
                frames > 0 && offset < data_size as u64,
                "Invalid FSB sample extent"
            );
            ensure!(
                u64::from(word(record, 4)?).abs_diff(frames * u64::from(channels) * 2) <= 4096,
                "FSB info sample size differs"
            );
            samples.push((id,json!({"id":id,"stream":i+1,"source_path":name,"frames":frames,"channels":channels,"sample_rate":rate,"data_offset":data_at+offset,"loop":looping})));
        }
        ensure!(cursor == headers.len(), "FSB sample header count differs");
        let mut entries = BTreeMap::new();
        for i in 0..samples.len() {
            let end = if i + 1 < samples.len() {
                samples[i + 1].1["data_offset"].as_u64().unwrap()
            } else {
                data_at + data_size as u64
            };
            let (id, ref mut sample) = samples[i];
            let start = sample["data_offset"].as_u64().unwrap();
            ensure!(end >= start, "FSB sample offsets are not ordered");
            sample["encoded_bytes"] = json!(end - start);
            ensure!(
                entries.insert(id, sample.clone()).is_none(),
                "Duplicate FSB sample identity"
            );
        }
        Ok(Self { path, entries })
    }
}

fn pcm_layout(bytes: &[u8]) -> Result<(u16, u32, u64)> {
    ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "Decoded audio is not RIFF WAVE"
    );
    ensure!(
        u64::from(word(bytes, 4)?) + 8 == bytes.len() as u64,
        "Decoded RIFF extent differs"
    );
    let mut at = 12;
    let mut format = None;
    let mut samples = None;
    while at + 8 <= bytes.len() {
        let size = word(bytes, at + 4)? as usize;
        let payload = bytes
            .get(at + 8..at + 8 + size)
            .context("Decoded RIFF chunk exceeds file")?;
        match &bytes[at..at + 4] {
            b"fmt " => ensure!(
                format.replace(payload).is_none(),
                "Duplicate decoded audio format"
            ),
            b"data" => ensure!(
                samples.replace(payload).is_none(),
                "Duplicate decoded audio samples"
            ),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    ensure!(at == bytes.len(), "Trailing incomplete decoded RIFF chunk");
    let format = format.context("Decoded audio format missing")?;
    let samples = samples.context("Decoded audio samples missing")?;
    ensure!(format.len() >= 16, "Short decoded audio format");
    let code = u16::from_le_bytes(format[..2].try_into()?);
    if code == 0xfffe {
        ensure!(
            format.len() >= 40
                && format[24..40] == [1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113],
            "Decoded extensible audio is not PCM"
        );
    } else {
        ensure!(code == 1, "Decoded audio is not PCM");
    }
    let channels = u16::from_le_bytes(format[2..4].try_into()?);
    let rate = word(format, 4)?;
    let align = u16::from_le_bytes(format[12..14].try_into()?);
    ensure!(
        (1..=32).contains(&channels) && align == channels * 2 && format[14..16] == [16, 0],
        "Decoded audio is not interleaved PCM16"
    );
    ensure!(
        u64::from(word(format, 8)?) == u64::from(rate) * u64::from(align)
            && samples.len() % usize::from(align) == 0,
        "Decoded PCM extent differs"
    );
    Ok((channels, rate, (samples.len() / usize::from(align)) as u64))
}

pub(super) fn export(
    options: &Options,
    behavior: &Value,
    pages: &mut Pages,
    root: &Path,
) -> Result<Value> {
    let banks = std::iter::once(&options.bank)
        .chain(&options.additional_banks)
        .map(|path| Bank::open(path))
        .collect::<Result<Vec<_>>>()?;
    let decoder = options.decoder.canonicalize()?;
    for bank in &banks {
        pages.record(&bank.path)?;
        pages.record(&bank.path.with_extension("fsb.info"))?;
    }
    let nodes = behavior["nodes"].as_object().context("Behavior nodes")?;
    let mut wanted = BTreeSet::new();
    for sound in nodes.values().filter(|n| n["tag"]["group"] == "snd!") {
        for variant in sound["data"]["pitch_ranges"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|r| r["variants"].as_array().into_iter().flatten())
        {
            wanted.insert(u32::try_from(
                variant["fsb_info"]
                    .as_u64()
                    .context("FSB sample identity")?,
            )?);
        }
    }
    fs::create_dir_all(root)?;
    let mut result = Vec::new();
    let mut unavailable = Vec::new();
    let expected = wanted.len();
    for id in wanted {
        crate::cancellation::check()?;
        let matches = banks
            .iter()
            .filter_map(|bank| bank.entries.get(&id).map(|sample| (bank, sample)))
            .collect::<Vec<_>>();
        if matches.is_empty() && options.record_missing_samples {
            unavailable.push(json!({"id":id,"reason":"Referenced source sample is absent from the configured banks","native_playback":false}));
            continue;
        }
        ensure!(
            matches.len() == 1,
            "Sample {id:08X} has {} matches across configured FSB banks. Configure its source bank and only one dialogue language.",
            matches.len()
        );
        let (bank, sample) = matches[0];
        let mut sample = sample.clone();
        sample["bank"] = json!(bank.path);
        let name = format!("{id:08X}.wav");
        let destination = root.join(&name);
        let mut command = Command::new(&decoder);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
            .args(["-i", "-s"])
            .arg(sample["stream"].as_u64().unwrap().to_string())
            .arg("-o")
            .arg(&destination)
            .arg(&bank.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = crate::cancellation::wait_with_output(
            command.spawn().context("Starting configured FSB decoder")?,
        )?;
        ensure!(
            output.status.success(),
            "FSB decode {id:08X}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (channels, rate, frames) = pcm_layout(&fs::read(&destination)?)?;
        ensure!(
            u64::from(channels) == sample["channels"].as_u64().unwrap()
                && u64::from(rate) == sample["sample_rate"].as_u64().unwrap()
                && frames == sample["frames"].as_u64().unwrap(),
            "Decoded FSB sample layout differs"
        );
        sample["file"] = json!(name);
        sample["output"] = serde_json::to_value(fingerprint(&destination)?)?;
        result.push(sample);
    }
    ensure!(
        expected == 0 || !result.is_empty(),
        "None of the required source audio samples are available in the configured banks"
    );
    let report = json!({"banks":banks.iter().map(|b|&b.path).collect::<Vec<_>>(),"decoder":{"path":decoder,"file":fingerprint(&decoder)?},"samples":result,"unavailable":unavailable,"playback":"Available source PCM preserved without substitution. Native routes are verified separately."});
    fs::write(root.join("audio.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

fn fire_sound(behavior: &Value) -> Result<Option<&Value>> {
    let edges = behavior["edges"].as_array().context("Behavior links")?;
    let nodes = behavior["nodes"].as_object().context("Behavior nodes")?;
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    let mut queue = edges
        .iter()
        .filter(|e| e["owner"] == root && e["role"] == "fire")
        .map(|e| e["tag"]["datum"].as_u64().context("Fire identity"))
        .collect::<Result<Vec<_>>>()?;
    let mut seen = BTreeSet::new();
    let mut sounds = Vec::new();
    while let Some(tag) = queue.pop() {
        if !seen.insert(tag) {
            continue;
        }
        let Some(node) = nodes.get(&format!("{tag:08X}")) else {
            continue;
        };
        if node["tag"]["group"] == "snd!" && node["data"]["class"] == 4 {
            sounds.push(node);
        }
        if node["tag"]["group"] == "effe" {
            for edge in edges
                .iter()
                .filter(|e| e["owner"] == tag && e["role"] == "effect_part")
            {
                queue.push(edge["tag"]["datum"].as_u64().context("Effect identity")?);
            }
        }
    }
    ensure!(
        sounds.len() <= 1,
        "Source barrel has multiple primary firing sounds. Explicit layer translation is required."
    );
    Ok(sounds.pop())
}

/// Fit primary source fire variations to a checked native firing route.
pub(super) fn native(
    reader: &mut crate::tiger::reader::Reader,
    item: u32,
    behavior: &Value,
    source: &Path,
    graph: &Path,
    options: &Options,
) -> Result<Value> {
    let rig = crate::tiger::rig::inspect_with_audio(reader, item, false)?;
    let Some(sound) = fire_sound(behavior)? else {
        return Ok(
            json!({"status":"source_only","reason":"No unique primary firing sound on the source barrel"}),
        );
    };
    let groups = rig["audio"]["unnamed_groups"]
        .as_array()
        .context("Native firing presentation unavailable")?;
    let mut candidates = groups
        .iter()
        .flat_map(|g| {
            g["sounds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |s| (g, s))
        })
        .filter(|(_, s)| {
            s["root_slot"] == 0
                && s["component_class"] == "808084E9"
                && s["node_kind"] == "once"
                && s["media"].as_array().is_some_and(|m| !m.is_empty())
        })
        .collect::<Vec<_>>();
    // The weapon's primary event carries the selected presentation content key.
    // Other once-nodes in the firing entity can provide mechanical or tail layers.
    if candidates
        .iter()
        .any(|(g, s)| s["event_id"] == g["content_key"])
    {
        candidates.retain(|(g, s)| s["event_id"] == g["content_key"]);
    }
    ensure!(
        candidates.len() == 1,
        "Native firing sound is absent or ambiguous"
    );
    let (presentation, native) = candidates[0];
    let output_bus = options
        .fire_bus
        .context("Configure an explicit native fire_bus for nonspatial primary-fire playback")?;
    let native_bank =
        u32::from_str_radix(native["bank"].as_str().context("Native firing bank")?, 16)?;
    verify_bus(&reader.tag(native_bank, None)?.0, output_bus)?;
    let ranges = sound["data"]["pitch_ranges"]
        .as_array()
        .context("Sound ranges")?;
    ensure!(
        ranges.len() == 1,
        "Primary firing sound requires pitch-range switching"
    );
    let variants = ranges[0]["variants"]
        .as_array()
        .context("Sound variations")?;
    ensure!(
        !variants.is_empty(),
        "Primary firing sound has no variations"
    );
    let mut media = Vec::new();
    let mut files = Vec::new();
    fs::create_dir_all(graph.join("audio"))?;
    for variant in variants {
        let id = u32::try_from(variant["fsb_info"].as_u64().context("Sound identity")?)?;
        let id = format!("{id:08X}");
        let file = format!("audio/{id}.wem");
        let pcm = crate::tiger::audio::transcode::normalize_pcm_wem(&fs::read(
            source.join(format!("{id}.wav")),
        )?)?;
        fs::write(graph.join(&file), pcm)?;
        media.push(id.clone());
        files.push(json!({"source_tag":id,"file":file}));
    }
    let ids = media
        .iter()
        .map(|id| u32::from_str_radix(id, 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let gain = sound["data"]["playback"]["gain_db"]
        .as_f64()
        .context("Source firing gain")? as f32;
    let bank = crate::tiger::audio::bank::Bank::one_shot(&ids, output_bus, gain)?;
    fs::write(
        graph.join("audio/fire.bank.json"),
        serde_json::to_vec(&bank)?,
    )?;
    let sound = json!({"source":sound["tag"],"source_playback":sound["data"]["playback"],"media":media,"media_ids":media,"bank":"00000001","event_id":"00000001","routing":"source_one_shot"});
    Ok(
        json!({"authoring_schema":3,"runtime_entity":rig["runtime_entity"],"matched_events":[],"firing_events":[{"sounds":[sound],"native_sounds":[native],"native_presentation":presentation,"mapping":"Source primary fire to unique native fire slot"}],"transcoded_media":files,"converted_banks":[{"source_tag":"00000001","file":"audio/fire.bank.json","version":113,"routing":"source_one_shot"}],"conversion_errors":[],"output_bus":output_bus,"limits":["Primary firing PCM, gain and variation count use one complete recording per native trigger. This explicit nonspatial route does not reproduce source attenuation, tails, pitch modulation, skip fractions or environment switches. Native trigger timing is retained."]}),
    )
}

fn verify_bus(bank: &[u8], bus: u32) -> Result<()> {
    let mut at = 0;
    let mut version = None;
    let mut found = false;
    while at < bank.len() {
        let size = word(bank, at + 4)? as usize;
        let body = bank
            .get(at + 8..at + 8 + size)
            .context("Native bank chunk exceeds payload")?;
        match &bank[at..at + 4] {
            b"BKHD" => version = Some(word(body, 0)?),
            b"HIRC" => {
                let mut cursor = 4;
                for _ in 0..word(body, 0)? {
                    let kind = *body.get(cursor).context("Native HIRC type")?;
                    let size = word(body, cursor + 1)? as usize;
                    let object = body
                        .get(cursor + 5..cursor + 5 + size)
                        .context("Native HIRC object exceeds chunk")?;
                    if matches!(kind, 2 | 5 | 6 | 7 | 9) {
                        let node = if kind == 2 { 18 } else { 4 };
                        if object.get(node..node + 3) == Some(&[0, 0, 0]) {
                            found |= word(object, node + 3)? == bus;
                        }
                    }
                    cursor += 5 + size;
                }
                ensure!(cursor == body.len(), "Native HIRC extent differs");
            }
            _ => {}
        }
        at += 8 + size;
    }
    ensure!(
        version == Some(113) && found,
        "Selected fire_bus is not a checked output bus in the native firing bank"
    );
    Ok(())
}
