//! Source cues, complete Wwise bank closures and PCM media for the shared graph linker.
use super::*;
use crate::tiger::{
    audio::{bank, transcode},
    reader::Reader,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
mod routing;
pub(super) use routing::prepare as prepare_named;

struct Cue {
    events: [u32; 2],
    settings: u32,
    bank: u32,
    media: Vec<u32>,
}

fn present(id: u32) -> bool {
    !matches!(id, 0 | u32::MAX | 0x811C9DC5)
}

impl Cue {
    fn read(source: &Source, tag: u32) -> Result<Self> {
        ensure!(
            source.reference(tag)? == 0x8080080A,
            "source audio reference is not a cue"
        );
        let p = source.read(tag)?;
        ensure!(
            p.u64(0)? == p.0.len() as u64
                && p.0
                    .get(0x10..0x30)
                    .is_some_and(|b| b.iter().all(|v| *v == 0))
                && p.u32(0x48)? == u32::MAX
                && p.u32(0x4C)? == 0,
            "source cue has unconverted spatial or playback fields"
        );
        let rows = p.array(0x38, 4, Some(0x80800014))?;
        ensure!(
            !rows.is_empty()
                && rows.len() <= 1024
                && p.pointer(0x40)? == 0x60
                && p.u32(0x5C)? == 0x80800184
                && p.0.len() == 0x70 + rows.len() * 4,
            "source cue media envelope differs"
        );
        let events = [p.u32(8)?, p.u32(12)?];
        ensure!(present(events[0]), "source cue has no primary event");
        Ok(Self {
            events,
            settings: p.u32(0x30)?,
            bank: p.u32(0x34)?,
            media: rows
                .into_iter()
                .map(|at| p.u32(at))
                .collect::<Result<_>>()?,
        })
    }
}

fn source_kind(source: &Source, tag: u32, kind: u8, subtype: u8) -> Result<()> {
    let entry = source
        .package(tag)?
        .reader
        .entry(TagHash(tag).entry_index() as usize)
        .context("source audio entry absent")?;
    ensure!(
        entry.file_type == kind && entry.file_subtype == subtype,
        "source audio storage differs at {tag:08X}"
    );
    Ok(())
}

fn source_bank(source: &Source, id: u32) -> Result<u32> {
    let subtype = if id == bank::hash("init") { 19 } else { 20 };
    let tags = source
        .classes(id)
        .into_iter()
        .filter(|tag| source_kind(source, *tag, 0, subtype).is_ok())
        .collect::<Vec<_>>();
    let tag = *tags.first().context("source bank dependency missing")?;
    let bytes = source.read(tag)?;
    for other in &tags[1..] {
        ensure!(
            source.read(*other)?.0 == bytes.0,
            "source bank identity has conflicting payloads"
        );
    }
    Ok(tag)
}

fn template(native: &Reader, kind: u8, subtype: u8) -> Result<u32> {
    native
        .manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .flat_map(|(&pkg, entries)| {
            entries
                .iter()
                .enumerate()
                .filter(move |(_, e)| e.file_type == kind && e.file_subtype == subtype)
                .map(move |(i, _)| TagHash::new(pkg, i as u16).0)
        })
        .min()
        .context("native audio storage template absent")
}

fn class_template(native: &Reader, class: u32) -> Result<u32> {
    native
        .classes(class)
        .into_iter()
        .min()
        .context("native audio class absent")
}

fn node(
    nodes: &mut Vec<Value>,
    files: &mut Vec<(String, Vec<u8>)>,
    symbol: String,
    template: u32,
    bytes: &[u8],
    patches: Vec<Value>,
) -> usize {
    let file = format!("{symbol}.bin");
    files.push((file.clone(), bytes.to_vec()));
    let index = nodes.len();
    nodes.push(json!({"symbol":symbol,"file":file,"template":template,"patches":patches}));
    index
}

/// A complete, validated source closure. Candidate talent choices can prepare their shared
/// namespace without publishing files or changing the graph's accepted audio section.
pub(super) struct Prepared {
    graph: Value,
    files: Vec<(String, Vec<u8>)>,
}

impl Prepared {
    pub(super) fn emit(self, folder: &Path) -> Result<Value> {
        fs::create_dir_all(folder)?;
        for (file, payload) in self.files {
            fs::write(folder.join(file), payload)?;
        }
        Ok(self.graph)
    }
}

fn symbol(kind: &str, tag: u32) -> String {
    format!("audio-{kind}-{tag:08X}")
}

pub(super) fn cue_symbol(tag: u32) -> String {
    symbol("cue", tag)
}

/// How prepared banks reach the game. Graph banks are bound into one shared namespace, so
/// every object they share must agree. A named cue's bank is instantiated privately beside
/// its selected native cue, so only the closure of that cue's own bank must be consistent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Graph,
    Named,
}

fn reachable(root: u32, dependencies: &BTreeMap<u32, BTreeSet<u32>>) -> BTreeSet<u32> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(tag) = pending.pop() {
        if found.insert(tag) {
            pending.extend(dependencies.get(&tag).into_iter().flatten().copied());
        }
    }
    found
}

fn prepare_banks(
    source: &Source,
    native: &mut Reader,
    cues: &BTreeMap<u32, Cue>,
    route: Route,
) -> Result<BTreeMap<u32, bank::Bank>> {
    let source_init = source.read(source_bank(source, bank::hash("init"))?)?;
    let source_settings = bank::settings::Settings::read(&source_init.0, 89)?;
    let init_tags = native
        .manager
        .get_all_by_reference(bank::hash("init"))
        .iter()
        .filter(|(_, e)| e.file_type == 26 && matches!(e.file_subtype, 4 | 5))
        .map(|(tag, _)| tag.0)
        .collect::<Vec<_>>();
    let mut native_init = None;
    for tag in init_tags {
        let p = native.tag(tag, None)?;
        let settings = bank::settings::Settings::read(&p.0, 113)?;
        let objects = bank::settings::objects(&p.0, 113)?;
        ensure!(
            native_init
                .as_ref()
                .is_none_or(|previous| previous == &(settings.clone(), objects.clone())),
            "native Init definitions disagree"
        );
        native_init = Some((settings, objects));
    }
    let (native_settings, native_objects) = native_init.context("native Init bank missing")?;
    let mut pending = cues.values().map(|cue| cue.bank).collect::<BTreeSet<_>>();
    let mut banks = BTreeMap::new();
    let mut dependencies = BTreeMap::new();
    while let Some(tag) = pending.pop_first() {
        if banks.contains_key(&tag) {
            continue;
        }
        ensure!(banks.len() < 256, "source audio bank closure exceeds limit");
        source_kind(source, tag, 0, 20)?;
        let bytes = source.read(tag)?;
        let converted = bank::upgrade_with_settings(&bytes.0, &source_settings, &native_settings)
            .with_context(|| format!("source audio bank {tag:08X}"))?;
        ensure!(
            converted.bank_id == source.reference(tag)?,
            "source bank storage identity differs"
        );
        let mut needed = BTreeSet::new();
        for id in converted.bank_dependencies() {
            needed.insert(source_bank(source, id)?);
        }
        pending.extend(needed.iter().copied());
        dependencies.insert(tag, needed);
        banks.insert(tag, converted);
    }
    // Source banks repeat parent containers with only their own children, so separately
    // instantiated banks may disagree on an object that a shared namespace would merge.
    let groups: Vec<(BTreeSet<u32>, Vec<&Cue>)> = match route {
        Route::Graph => vec![(banks.keys().copied().collect(), cues.values().collect())],
        Route::Named => cues
            .values()
            .map(|cue| (reachable(cue.bank, &dependencies), vec![cue]))
            .collect(),
    };
    for (tags, members) in groups {
        let closure = tags
            .iter()
            .map(|tag| banks[tag].clone())
            .collect::<Vec<_>>();
        let owned = closure
            .iter()
            .flat_map(|b| b.objects.iter().copied())
            .collect::<BTreeSet<_>>();
        for bank in &closure {
            ensure!(
                bank.external_objects()
                    .iter()
                    .all(|id| owned.contains(id) || native_objects.contains(id)),
                "source bank references unavailable native global objects"
            );
        }
        let namespace = bank::Namespace::new("source-audio-validation", &closure)?;
        for cue in members {
            for id in cue.events.into_iter().filter(|id| present(*id)) {
                namespace.object(id)?;
            }
        }
    }
    Ok(banks)
}

type Media = BTreeMap<u32, (u32, Vec<u8>)>;

fn prepare_media(
    source: &Source,
    cues: &BTreeMap<u32, Cue>,
    banks: &BTreeMap<u32, bank::Bank>,
) -> Result<Media> {
    let mut media = BTreeMap::<u32, (u32, Vec<u8>)>::new();
    for tag in cues
        .values()
        .flat_map(|cue| cue.media.iter().copied())
        .filter(|tag| *tag != u32::MAX)
    {
        source_kind(source, tag, 8, 21)?;
        let id = source.reference(tag)?;
        let bytes = source.read(tag)?.0;
        if let Some((_, previous)) = media.get(&id) {
            ensure!(
                *previous == bytes,
                "source media identity has conflicting payloads"
            );
        } else {
            media.insert(id, (tag, bytes));
        }
    }
    for bank in banks.values() {
        for (_, id) in bank.media_fields() {
            ensure!(
                media.contains_key(&id),
                "bank medium {id:08X} is outside the source cue closure"
            );
        }
    }
    let mut converted_media = BTreeMap::new();
    for (&id, (tag, bytes)) in &media {
        converted_media.insert(
            id,
            (
                *tag,
                transcode::early_pcm_wem(bytes)
                    .with_context(|| format!("source medium {tag:08X}"))?,
            ),
        );
    }
    Ok(converted_media)
}

/// Emit only after every bank, cue and medium in the requested closure has validated.
pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    folder: &Path,
    tags: &BTreeSet<u32>,
) -> Result<Value> {
    prepare_route(source, native, folder, tags, Route::Graph)
}

/// Validate the complete graph namespace and retain its payloads without publishing files.
pub(super) fn plan(source: &Source, native: &mut Reader, tags: &BTreeSet<u32>) -> Result<Prepared> {
    plan_route(source, native, tags, Route::Graph)
}

/// Graph routes emit private profiles with a checked native witness. Named routes emit only
/// banks and media, because install clones each selected native cue with its own profile.
fn prepare_route(
    source: &Source,
    native: &mut Reader,
    folder: &Path,
    tags: &BTreeSet<u32>,
    route: Route,
) -> Result<Value> {
    plan_route(source, native, tags, route)?.emit(folder)
}

fn plan_route(
    source: &Source,
    native: &mut Reader,
    tags: &BTreeSet<u32>,
    route: Route,
) -> Result<Prepared> {
    ensure!(!tags.is_empty(), "source audio cue set is empty");
    let cues = tags
        .iter()
        .map(|&tag| Ok((tag, Cue::read(source, tag)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let banks = prepare_banks(source, native, &cues, route)?;
    let converted_media = prepare_media(source, &cues, &banks)?;
    let mut settings = BTreeMap::new();
    if route == Route::Graph {
        let mut profiles = Vec::new();
        for tag in native
            .classes(0x80808E68)
            .into_iter()
            .collect::<BTreeSet<_>>()
        {
            profiles.push((tag, native.tag(tag, Some(0x80808E68))?));
        }
        for cue in cues.values() {
            if settings.contains_key(&cue.settings) {
                continue;
            }
            ensure!(
                source.reference(cue.settings)? == 0x80800714,
                "source cue settings class differs"
            );
            let p = source.read(cue.settings)?;
            ensure!(
                p.0.len() == 24 && p.u64(0)? == 24,
                "source cue settings envelope differs"
            );
            let (tag, _) = profiles
                .iter()
                .find(|(_, native)| native.0 == p.0)
                .or_else(|| {
                    // Original 7E5F80 and 10E6B30 expose +0C to the spatial-limit
                    // consumers. Every other field still needs a byte-identical witness.
                    // Exact profiles retain their existing acceptance, including any
                    // nonfinite native values. Only new finite limits use this route.
                    p.f32(12)
                        .ok()
                        .filter(|limit| limit.is_finite())
                        .and_then(|_| {
                            profiles.iter().find(|(_, native)| {
                                native.0.len() == 24
                                    && native.0[..12] == p.0[..12]
                                    && native.0[16..] == p.0[16..]
                                    && native.f32(12).is_ok_and(|limit| limit.is_finite())
                            })
                        })
                })
                .context(
                    "source acoustic settings have no native witness outside the spatial limit",
                )?;
            // The stock record supplies the native class and unchanged-field witness.
            // The emitted private payload preserves the source limit without clamping.
            settings.insert(cue.settings, (*tag, Arc::new(p)));
        }
    }
    let mut nodes = Vec::new();
    let mut files = Vec::new();
    let media_template = template(native, 26, 6)?;
    let bank_template = template(native, 26, 5)?;
    for (&id, (tag, bytes)) in &converted_media {
        let index = node(
            &mut nodes,
            &mut files,
            symbol("media", *tag),
            media_template,
            bytes,
            Vec::new(),
        );
        nodes[index]["storage"] = json!("audio_media");
        nodes[index]["source_media_id"] = json!(id);
        nodes[index]["source_media_tag"] = json!(tag);
    }
    for (&tag, bank) in &banks {
        let index = node(
            &mut nodes,
            &mut files,
            symbol("bank", tag),
            bank_template,
            &bank.bytes,
            Vec::new(),
        );
        nodes[index]["storage"] = json!("audio_bank");
        nodes[index]["source_bank_tag"] = json!(tag);
        nodes[index]["audio_bank"] = serde_json::to_value(bank)?;
        nodes[index]["audio_media"] = json!(
            bank.media_fields()
                .into_iter()
                .map(|(_, id)| id)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|id| json!({"id":id,"symbol":symbol("media", converted_media[&id].0)}))
                .collect::<Vec<_>>()
        );
    }
    if route == Route::Graph {
        plan_cues(
            source,
            native,
            &mut nodes,
            &mut files,
            &cues,
            &settings,
            &converted_media,
        )?;
    }
    Ok(Prepared {
        graph: json!({"nodes":nodes,"cues":tags,"banks":banks.len(),"media":converted_media.len(),
        "source_builds":source.report.builds,"source_stamp":source.report.stamp,"gameplay_verified":false}),
        files,
    })
}

/// Graph cues reference their emitted settings, bank, auxiliary record and media by symbol.
fn plan_cues(
    source: &Source,
    native: &mut Reader,
    nodes: &mut Vec<Value>,
    files: &mut Vec<(String, Vec<u8>)>,
    cues: &BTreeMap<u32, Cue>,
    settings: &BTreeMap<u32, (u32, Arc<Payload>)>,
    converted_media: &Media,
) -> Result<()> {
    let cue_template = class_template(native, 0x80809802)?;
    let auxiliary_template = class_template(native, 0x80808E69)?;
    let mut auxiliary = vec![0; 40];
    auxiliary[..8].copy_from_slice(&40u64.to_le_bytes());
    node(
        nodes,
        files,
        "audio-auxiliary".into(),
        auxiliary_template,
        &auxiliary,
        Vec::new(),
    );
    for (&tag, (template, p)) in settings {
        node(
            nodes,
            files,
            symbol("settings", tag),
            *template,
            &p.0,
            Vec::new(),
        );
    }
    for (&tag, cue) in cues {
        let mut bytes = vec![0u8; 0x50 + cue.media.len() * 4];
        let length = bytes.len() as u64;
        bytes[..8].copy_from_slice(&length.to_le_bytes());
        bytes[8..12].copy_from_slice(&cue.events[0].to_le_bytes());
        bytes[12..16].copy_from_slice(&cue.events[1].to_le_bytes());
        for at in [0x18, 0x40] {
            bytes[at..at + 8].copy_from_slice(&(cue.media.len() as u64).to_le_bytes());
        }
        bytes[0x20..0x28].copy_from_slice(&0x20u64.to_le_bytes());
        bytes[0x3C..0x40].copy_from_slice(&0x80809FBDu32.to_le_bytes());
        bytes[0x48..0x4C].copy_from_slice(&0x80800014u32.to_le_bytes());
        let mut patches = Vec::new();
        let mut patch = |at: usize, symbol: String| {
            bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            patches.push(json!({"offset":at,"symbol":symbol}));
        };
        patch(0x10, symbol("settings", cue.settings));
        patch(0x14, symbol("bank", cue.bank));
        patch(0x28, "audio-auxiliary".into());
        for (i, medium) in cue.media.iter().enumerate() {
            if *medium != u32::MAX {
                patch(
                    0x50 + i * 4,
                    symbol("media", converted_media[&source.reference(*medium)?].0),
                );
            }
        }
        for (i, medium) in cue.media.iter().enumerate() {
            if *medium == u32::MAX {
                bytes[0x50 + i * 4..0x54 + i * 4].fill(255);
            }
        }
        let index = node(nodes, files, cue_symbol(tag), cue_template, &bytes, patches);
        nodes[index]["source_cue_tag"] = json!(tag);
        nodes[index]["audio_events"] = json!(
            cue.events
                .iter()
                .enumerate()
                .filter(|(_, id)| present(**id))
                .map(|(i, id)| json!({"offset":8 + i * 4,"id":id}))
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

impl Source {
    /// Prepare a source cue closure as native graph nodes, with no installed-content writes.
    pub fn prepare_audio(
        &self,
        cues: &[u32],
        native_packages: &Path,
        output: &Path,
    ) -> Result<PathBuf> {
        let output = self.output(output)?;
        let output = crate::io::outside(
            &output,
            native_packages.parent().context("native installation")?,
        )?;
        let mut native = Reader::new(native_packages, &output.join("native-source"), false)?;
        let report = prepare(self, &mut native, &output, &cues.iter().copied().collect())?;
        let path = output.join("audio-graph.json");
        crate::io::write_json(&path, &report)?;
        native.finish()?;
        self.unchanged()?;
        Ok(path)
    }
}
