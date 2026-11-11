//! Graphs a talent attaches whole, translated with every graph they create into one imported
//! attachment group.
//!
//! This covers every attachment to the event's other combatant and any attachment with more
//! owners than the ordinary status, connected, nested, sequence and modifier forms, whatever its
//! target. Rise of Iron `56578.17.04.12.1251.evil` and Shadowkeep
//! `86657.20.08.23.1800.d2_rc___release` both build these graphs from shared framework owners
//! (network, effect context, category, invisibility), private effect sequences and sometimes a
//! bank, status, Duration host or modifier bank. In the victim explosion form, the sequence's
//! area query damages the victim's neighbours through a private outgoing profile. Other forms
//! create graphs of their own, such as Outbreak Prime's Virulence swarm, heal their host, as Red
//! Death does, or make it invisible, as Patience and Time does. The shared effect translator takes
//! any owner families a native template provides. The group's root replaces native graph
//! `8152BB5C` on the action.
//! Presentation events without a converter keep their timing as reported delays, as the ability
//! path does.
use super::*;
use crate::{
    d1_roi::{
        audio as graph_audio,
        dependency::{self, resource as resources},
        effect,
        native::damage_profile,
        sequence::Plan,
    },
    presentation::put,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The native graph the action compiles against before the imported root replaces it.
pub(super) const TEMPLATE: u32 = 0x8152BB5C;

/// Native sequences whose event instances supply the qualified envelopes: query, creation,
/// delay and sound (`8152BB5D`), particles (`80EFAEAE`) and presentation (`80F2E9FF`).
const ENVELOPES: [u32; 3] = [0x8152BB5D, 0x80EFAEAE, 0x80F2E9FF];

/// Stock resources of each translated resource class, used as package templates. Feedback member
/// `80C70634` has the layout of the Explosive Rounds member it templates. Grouped presentation
/// table `80BF9D65` belongs to `80BC57DB`, Bungie's port of the Incendiary explosion sequence.
const TEMPLATES: [(u32, u32); 7] = [
    (0x80809C36, 0x8152BB69),
    (0x80803EB0, 0x80C23C06),
    (0x80803E94, 0x80C23A42),
    (0x80803EA5, 0x80C23C07),
    (0x80803EAA, 0x80C23C08),
    (0x80803E99, 0x80C70634),
    (0x80808BCD, 0x80BF9D65),
];

pub(crate) struct Group {
    pub directory: PathBuf,
    pub sha256: String,
    pub symbol: String,
}

/// Event kinds kept as delays. Queries, creation and profile events translate, since a profile
/// event applies gameplay such as Red Death's kill heal. Presentation kind 31 has no converter.
/// Kind 3 places a managed sound emitter at the host, registered beside sound events
/// (`007E91AF`). Its `80800874` wrapper holds a looping cue and fields with no traced native
/// counterpart, so a talent's graph keeps its timing without the sound.
fn omitted() -> BTreeSet<(u32, u32)> {
    let mut kinds = effect::OMITTED
        .into_iter()
        .filter(|kind| !matches!(kind, (0x808003BB, 10) | (0x808028A6, 26) | (0x80802402, 24)))
        .collect::<BTreeSet<_>>();
    kinds.insert((0x80801AA2, 31));
    kinds.insert((0x80801EF1, 3));
    kinds
}

/// Created graphs the effect translator cannot build, with the stock graph that the native port
/// of their creating sequence makes from the same events. Native sequence `80B75177` ports
/// Virulence sequence `80A88C77`, keeping its event names and values, and creates `80B9E4FB` where
/// the source creates swarm projectile `80A88D5B`, whose model, movement and tracking owners
/// belong to the projectile route rather than effect graphs. The swarm then keeps Bungie's port,
/// as Outbreak Prime's projectile keeps native nanite spawn `81533027`.
///
/// Native sequence `80C1960F` ports overshield creation sequences `80A7E28B`, `80A7E2E2`,
/// `80A7E2E3` and `80A7E2E4`, which share its controls, delays, event names and parameters. Each
/// source sequence's first creation event makes a 75-vitality overshield graph and its second
/// another overshield graph, the pairs `80A6A517/518`, `80A6A55B/55C`, `80A6A55D/55E` and
/// `80A6A55F/560`. The port's two events make `80BB74DC` and `80BB74E3`, which hold the same HUD
/// status `F2E559CB` with key `0D3C3FB0`, overshield sequence `81579597` and controller component
/// `80FEF6FD`, the native form of the source's `80802A94` owner running a perk action. That action
/// cannot be built inside an effect graph, so each source graph keeps the port's graph made by the
/// same event. The port gives the second graph 75 vitality where the source gives 95.
const PORTED: [(u32, u32); 9] = [
    (0x80A88D5B, 0x80B9E4FB),
    (0x80A6A517, 0x80BB74DC),
    (0x80A6A55B, 0x80BB74DC),
    (0x80A6A55D, 0x80BB74DC),
    (0x80A6A55F, 0x80BB74DC),
    (0x80A6A518, 0x80BB74E3),
    (0x80A6A55C, 0x80BB74E3),
    (0x80A6A55E, 0x80BB74E3),
    (0x80A6A560, 0x80BB74E3),
];

fn ported(graph: u32) -> Option<u32> {
    PORTED
        .iter()
        .find_map(|&(source, stock)| (source == graph).then_some(stock))
}

/// The attached graph and every graph its creation events make or its condition volumes give,
/// outermost first. A ported graph is created as its stock counterpart, so its own creations stay
/// with that graph.
fn created(source: &Source, root: u32, omitted: &BTreeSet<(u32, u32)>) -> Result<Vec<u32>> {
    let mut order = Vec::new();
    let mut stack = vec![(root, 0)];
    while let Some((tag, depth)) = stack.pop() {
        if order.contains(&tag) {
            continue;
        }
        ensure!(
            depth < 4,
            "Source created graphs nest beyond the translation limit"
        );
        order.push(tag);
        for sequence in effect::sequences(source, tag)? {
            let plan = Plan::read_omitting(source, &source.read(sequence)?, omitted)?;
            for graph in plan.graphs() {
                if ported(graph).is_none() {
                    stack.push((graph, depth + 1));
                }
            }
        }
        for graph in effect::recipients(source, tag)? {
            stack.push((graph, depth + 1));
        }
    }
    Ok(order)
}

/// The private symbol of the attached graph or one of its created graphs.
fn symbol(prefix: &str, root: u32, tag: u32) -> String {
    if tag == root {
        format!("{prefix}-root")
    } else {
        format!("{prefix}-graph-{tag:08X}")
    }
}

/// Translations the sequences name besides their own owners: created graphs by symbol, and
/// private profiles and feedback with their assets and the profiles' audio cues.
struct Named {
    resources: effect::Resources,
    assets: BTreeMap<String, dependency::Asset>,
    cues: BTreeSet<u32>,
    /// Each area profile's source words without a qualified role.
    words: Vec<Value>,
}

fn named(
    source: &Source,
    native: &mut Reader,
    shared: &effect::Shared,
    prefix: &str,
    closure: &[u32],
) -> Result<Named> {
    let root = closure[0];
    let (mut graphs, profiles, feedback) = shared.named_resources();
    // Condition volumes name the graphs they give by symbol as creation events do.
    for &graph in closure {
        graphs.extend(effect::recipients(source, graph)?);
    }
    let mut named = Named {
        resources: effect::Resources::default(),
        assets: BTreeMap::new(),
        cues: BTreeSet::new(),
        words: Vec::new(),
    };
    for tag in profiles {
        let translated = damage_profile::translate(source, native, tag, prefix)?;
        named
            .resources
            .profiles
            .insert(tag, translated.assets[0].symbol.clone());
        named.cues.extend(translated.cues);
        named
            .words
            .push(json!({"profile":tag,"source_90":translated.unqualified[0],
            "source_A8":translated.unqualified[1],"absent_categories":translated.absent,
            "silent_cues":translated.silent}));
        for asset in translated.assets {
            named.assets.entry(asset.symbol.clone()).or_insert(asset);
        }
    }
    for tag in feedback {
        let translated = damage_profile::feedback::translate(source, tag, prefix)?;
        named
            .resources
            .feedback
            .insert(tag, translated[0].symbol.clone());
        for asset in translated {
            named.assets.entry(asset.symbol.clone()).or_insert(asset);
        }
    }
    for tag in graphs {
        if let Some(stock) = ported(tag) {
            named.resources.stock_graphs.insert(tag, stock);
        } else {
            named
                .resources
                .graphs
                .insert(tag, symbol(prefix, root, tag));
        }
    }
    Ok(named)
}

/// Every translated graph's pending resources, HUD dependencies and evidence.
struct Translated {
    pending: Vec<effect::Pending>,
    dependencies: BTreeMap<String, dependency::Asset>,
    evidence: Vec<Value>,
    /// The group's HUD bindings: each status whose HUD identity has no native row, with its
    /// source display, which the build gives a private entry as for an ability's group.
    hud: Vec<Value>,
}

/// `detached` names the root graph's modifier bank that the attaching action carries as a
/// separate attachment.
fn translate(
    source: &Source,
    native: &mut Reader,
    shared: &effect::Shared,
    graphs: &[u32],
    prefix: &str,
    detached: Option<u32>,
) -> Result<Translated> {
    let envelopes = ENVELOPES
        .iter()
        .map(|&tag| Ok((*native.tag(tag, Some(0x80809C36))?).clone()))
        .collect::<Result<Vec<_>>>()?;
    let mut out = Translated {
        pending: Vec::new(),
        dependencies: BTreeMap::new(),
        evidence: Vec::new(),
        hud: Vec::new(),
    };
    for (index, &tag) in graphs.iter().enumerate() {
        let base = 0x7C00_0000u32 + u32::try_from(index)? * 0x100;
        let name = symbol(prefix, graphs[0], tag);
        let graph = effect::translate_without(
            source,
            native,
            shared,
            &envelopes,
            tag,
            &name,
            base,
            detached.filter(|_| index == 0),
        )
        .with_context(|| format!("Attached graph {tag:08X}"))?;
        // Retained stock owners keep their tags. Temporary identities must be unused.
        ensure!(
            graph
                .components
                .iter()
                .all(|&id| id >> 24 == 0x80 || native.reference(id).is_err()),
            "Temporary attached identity collides with a native resource"
        );
        // A status whose HUD identity has no native row takes a private entry through the
        // group's HUD bindings, in any graph of the closure. Statuses with a native row keep it.
        out.hud.extend(graph.hud.into_iter().filter(|binding| {
            binding["name"]
                .as_u64()
                .and_then(|name| u32::try_from(name).ok())
                .is_some_and(|name| graph.private_hud.contains(&name))
        }));
        effect::merge(&mut out.dependencies, graph.dependencies)?;
        out.evidence
            .push(json!({"source":tag,"symbol":graph.symbol,"translation":graph.evidence}));
        out.pending.extend(graph.pending);
    }
    Ok(out)
}

/// Typed assets for the whole closure and the stock template of each.
fn assets(
    native: &Reader,
    translated: Translated,
    named: BTreeMap<String, dependency::Asset>,
    external: &BTreeMap<String, u32>,
) -> Result<(Vec<dependency::Asset>, BTreeMap<String, u32>)> {
    let mut classes = external.clone();
    for entry in &translated.pending {
        classes.insert(entry.symbol.clone(), native.reference(entry.template)?);
    }
    for asset in named.values().chain(translated.dependencies.values()) {
        classes.insert(asset.symbol.clone(), asset.class);
    }
    let mut templates = BTreeMap::new();
    let mut assets = Vec::new();
    for entry in translated.pending {
        let mut payload = entry.payload;
        let patches = entry
            .patches
            .into_iter()
            .map(|(offset, symbol)| {
                put(&mut payload.0, offset, &u32::MAX.to_le_bytes())?;
                Ok(dependency::Patch {
                    offset,
                    class: *classes
                        .get(&symbol)
                        .with_context(|| format!("Attached dependency {symbol} is absent"))?,
                    symbol,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        templates.insert(entry.symbol.clone(), entry.template);
        assets.push(dependency::Asset {
            class: classes[&entry.symbol],
            symbol: entry.symbol,
            payload,
            patches,
        });
    }
    let hud = native.classes(0x80803F26).into_iter().min();
    for asset in named
        .into_values()
        .chain(translated.dependencies.into_values())
    {
        let template = match TEMPLATES.iter().find(|(class, _)| *class == asset.class) {
            Some(&(_, template)) => template,
            None if asset.class == 0x80803F26 => {
                hud.context("Native HUD override template is absent")?
            }
            None => anyhow::bail!(
                "Attached resource class {:08X} has no template",
                asset.class
            ),
        };
        templates.insert(asset.symbol.clone(), template);
        assets.push(asset);
    }
    Ok((assets, templates))
}

/// A translated graph closure: typed assets with their stock templates, particle nodes, the
/// cues its sound events and profiles name, provenance and the root symbol.
pub(crate) struct Closure {
    pub assets: Vec<dependency::Asset>,
    pub templates: BTreeMap<String, u32>,
    pub nodes: Vec<resources::Node>,
    pub cues: BTreeSet<u32>,
    pub provenance: Value,
    pub root: String,
    /// HUD bindings for the group manifest's `hud_statuses`.
    pub hud: Vec<Value>,
}

/// Translate a graph and every graph its creation events make, with their profiles, feedback,
/// particle systems and cue closure. Attached graphs and projectile spawns share this form:
/// framework owners, a private effect sequence and sometimes a receiver or bank.
/// `private_hud` says whether the closure's statuses may name a HUD identity without a native
/// row, which the attachment group's HUD bindings then give a private entry.
pub(crate) fn translate_closure(
    source: &Source,
    native: &mut Reader,
    root: u32,
    prefix: &str,
    private_hud: bool,
) -> Result<Closure> {
    translate_closure_without(source, native, root, prefix, private_hud, None)
}

/// [`translate_closure`] with the root graph's detached modifier bank left out.
fn translate_closure_without(
    source: &Source,
    native: &mut Reader,
    root: u32,
    prefix: &str,
    private_hud: bool,
    detached: Option<u32>,
) -> Result<Closure> {
    let omitted = omitted();
    let graphs = created(source, root, &omitted)?;
    let groups = graphs
        .iter()
        .map(|&tag| effect::sequences(source, tag))
        .collect::<Result<Vec<_>>>()?;
    let mut shared = effect::Shared::prepare_omitting(source, native, &groups, prefix, &omitted)?;
    shared.private_hud = private_hud;
    let named = named(source, native, &shared, prefix, &graphs)?;
    shared.set_resources(named.resources);
    let words = named.words;
    let translated = translate(source, native, &shared, &graphs, prefix, detached)?;

    // One cue closure covers the sequences' sound events and the profiles' audio responses.
    let cues = shared
        .cues()
        .into_iter()
        .chain(named.cues)
        .collect::<BTreeSet<_>>();
    let external = shared
        .systems
        .iter()
        .map(|system| (system.clone(), 0x80806E28))
        .chain(
            cues.iter()
                .map(|&cue| (graph_audio::cue_symbol(cue), 0x80809802)),
        )
        .collect::<BTreeMap<_, _>>();
    let evidence = translated.evidence.clone();
    let hud = translated.hud.clone();
    let (assets, templates) = assets(native, translated, named.assets, &external)?;
    let root_symbol = symbol(prefix, root, root);
    let closure =
        dependency::validate_external(&assets, std::slice::from_ref(&root_symbol), &external)?;
    let referenced = assets
        .iter()
        .flat_map(|asset| &asset.patches)
        .filter(|patch| patch.class == 0x80806E28)
        .map(|patch| patch.symbol.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let nodes = resources::closure(shared.nodes, &referenced);
    if !nodes.is_empty() {
        resources::validate(&nodes, &referenced)?;
    }
    source.unchanged()?;
    let provenance = json!({
        "source":"d1_roi","root_graph":root,"native_template":TEMPLATE,
        "graphs":evidence,"shared_resources":shared.evidence,"closure":closure,
        "profile_words_without_roles":words,
        "gameplay_verified":false,
        "limits":["Shared network, effect context and category owners keep the stock Shadowkeep owners of the same role.",
            "Query action programs and native-only query fields take stock values, as recorded in the query converter.",
            "Presentation events without a converter keep their timing as native delays."]
    });
    Ok(Closure {
        assets,
        templates,
        nodes,
        cues,
        provenance,
        root: root_symbol,
        hud,
    })
}

/// Translate the attached graph and its whole resource closure, then save it as an imported
/// attachment group under `folder`. Choices of one import that attach the same source graph
/// share its prepared group. `detached` is the root's modifier bank the caller attaches
/// separately, which [`effect::detachable_modifier`] decides from the graph alone.
pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    root: u32,
    folder: &Path,
    detached: Option<u32>,
) -> Result<Group> {
    let prefix = format!("d1-closure-{root:08X}");
    let directory = group_folder(folder, root, &prefix);
    if directory.join("asset-graph.json").is_file() {
        let directory = directory.canonicalize()?;
        return Ok(Group {
            sha256: fingerprint(&directory)?,
            directory,
            symbol: symbol(&prefix, root, root),
        });
    }
    let closure = translate_closure_without(source, native, root, &prefix, true, detached)?;
    let audio = if closure.cues.is_empty() {
        None
    } else {
        Some(graph_audio::plan(source, native, &closure.cues)?.into_parts())
    };
    save(
        &directory,
        &Contents {
            assets: &closure.assets,
            templates: &closure.templates,
            nodes: &closure.nodes,
            audio,
            provenance: closure.provenance,
            root: closure.root,
            hud: &closure.hud,
        },
    )
}

/// Translate a source outgoing profile a direct effect names, with its feedback and cue closure,
/// and save it as an imported group whose root is the private profile. The area profile
/// translator writes it, since direct and area profiles share the source class `80AAC28B` and the
/// native class `80804B02`. Choices of one import naming the same profile share its group.
pub(super) fn prepare_profile(
    source: &Source,
    native: &mut Reader,
    tag: u32,
    folder: &Path,
) -> Result<Group> {
    let prefix = format!("d1-profile-{tag:08X}");
    let directory = group_folder(folder, tag, &prefix);
    let translated = damage_profile::translate(source, native, tag, &prefix)?;
    let root = translated
        .assets
        .first()
        .context("Translated profile has no root")?
        .symbol
        .clone();
    if directory.join("asset-graph.json").is_file() {
        let directory = directory.canonicalize()?;
        return Ok(Group {
            sha256: fingerprint(&directory)?,
            directory,
            symbol: root,
        });
    }
    let cues = translated.cues;
    let external = cues
        .iter()
        .map(|&cue| (graph_audio::cue_symbol(cue), 0x80809802))
        .collect::<BTreeMap<_, _>>();
    let named = translated
        .assets
        .into_iter()
        .map(|asset| (asset.symbol.clone(), asset))
        .collect::<BTreeMap<_, _>>();
    let empty = Translated {
        pending: Vec::new(),
        dependencies: BTreeMap::new(),
        evidence: Vec::new(),
        hud: Vec::new(),
    };
    let (assets, templates) = assets(native, empty, named, &external)?;
    let closure = dependency::validate_external(&assets, std::slice::from_ref(&root), &external)?;
    let audio = if cues.is_empty() {
        None
    } else {
        Some(graph_audio::plan(source, native, &cues)?.into_parts())
    };
    source.unchanged()?;
    save(
        &directory,
        &Contents {
            assets: &assets,
            templates: &templates,
            nodes: &[],
            audio,
            provenance: json!({"source":"d1_roi","root_profile":tag,
                "native_template":damage_profile::TEMPLATE,"closure":closure,
                "profile_words_without_roles":[translated.unqualified[0], translated.unqualified[1]],
                "absent_categories":translated.absent,"silent_cues":translated.silent,
                "gameplay_verified":false}),
            root,
            hud: &[],
        },
    )
}

/// A cue closure's manifest section and its payload files.
type Audio = (Value, Vec<(String, Vec<u8>)>);

struct Contents<'a> {
    assets: &'a [dependency::Asset],
    templates: &'a BTreeMap<String, u32>,
    nodes: &'a [resources::Node],
    audio: Option<Audio>,
    provenance: Value,
    root: String,
    hud: &'a [Value],
}

/// Where an import keeps the prepared group of source graph `tag`. Within the imports layout it is
/// `imports/d1_roi/<TAG>/perk-<converter revision>`, one folder every item naming that graph
/// reuses, so the graph converts once and the build sees one group. The library keeps it while a
/// recipe names it. Outside the layout, as in a test's folder, it is `attachments/<prefix>` in the
/// item's own conversion.
fn group_folder(folder: &Path, tag: u32, prefix: &str) -> PathBuf {
    crate::graph::importer_folder(folder, "d1_roi").map_or_else(
        || folder.join("attachments").join(prefix),
        |importer| {
            importer
                .join(format!("{tag:08X}"))
                .join(format!("perk-{}", crate::graph::CONVERTER_REVISION))
        },
    )
}

/// The group already prepared in `directory`, by another item's import or an earlier choice.
fn prepared(directory: &Path, symbol: String) -> Result<Group> {
    let directory = directory.canonicalize()?;
    Ok(Group {
        sha256: fingerprint(&directory)?,
        directory,
        symbol,
    })
}

/// Write the group into a folder of its own beside `directory` and move it into place, so a
/// reader never sees a partial group. Another import preparing the same group at once may move
/// its copy first, and that copy is then the group.
fn save(directory: &Path, contents: &Contents<'_>) -> Result<Group> {
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if directory.join("asset-graph.json").is_file() {
        return prepared(directory, contents.root.clone());
    }
    let parent = directory
        .parent()
        .context("Attachment group folder has no parent")?;
    fs::create_dir_all(parent)?;
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .context("Attachment group folder name")?;
    let staging = parent.join(format!(
        "~{name}-{}-{}",
        std::process::id(),
        WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let written = write(&staging, contents);
    let moved = written.and_then(|()| fs::rename(&staging, directory).map_err(Into::into));
    if let Err(error) = moved {
        let _ = fs::remove_dir_all(&staging);
        if !directory.join("asset-graph.json").is_file() {
            return Err(error);
        }
    }
    prepared(directory, contents.root.clone())
}

/// Write payloads, provenance and audio, then the manifest, which is the commit marker.
fn write(directory: &Path, contents: &Contents<'_>) -> Result<()> {
    ensure!(
        !directory.exists(),
        "Attachment group output already exists"
    );
    fs::create_dir_all(directory)?;
    let mut nodes = Vec::new();
    for asset in contents.assets {
        let file = format!("{}.bin", asset.symbol);
        fs::write(directory.join(&file), &asset.payload.0)?;
        let patches = asset
            .patches
            .iter()
            .map(|patch| json!({"offset":patch.offset,"symbol":patch.symbol}))
            .collect::<Vec<_>>();
        nodes.push(
            json!({"symbol":asset.symbol,"template":contents.templates[&asset.symbol],
            "file":file,"patches":patches}),
        );
    }
    for node in contents.nodes {
        let file = format!("{}.bin", node.symbol);
        fs::write(directory.join(&file), &node.payload)?;
        let patches = node
            .patches
            .iter()
            .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
            .collect::<Vec<_>>();
        nodes.push(
            json!({"symbol":node.symbol,"template":node.template,"file":file,
            "reference":node.reference,"patches":patches}),
        );
    }
    fs::write(
        directory.join("provenance.json"),
        serde_json::to_vec_pretty(&contents.provenance)?,
    )?;
    let mut manifest =
        json!({"attachments":{"installable":true,"roots":[contents.root],"nodes":nodes}});
    if let Some((section, files)) = &contents.audio {
        manifest["audio_assets"] = audio(directory, section.clone(), files)?;
    }
    if !contents.hud.is_empty() {
        manifest["hud_statuses"] = Value::Array(contents.hud.to_vec());
    }
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

/// The cue closure's files under `audio-assets`, with manifest paths relative to the group.
fn audio(directory: &Path, mut section: Value, files: &[(String, Vec<u8>)]) -> Result<Value> {
    let folder = directory.join("audio-assets");
    fs::create_dir_all(&folder)?;
    for (file, payload) in files {
        fs::write(folder.join(file), payload)?;
    }
    for node in section["nodes"]
        .as_array_mut()
        .context("Attachment group audio nodes are absent")?
    {
        let file = node["file"]
            .as_str()
            .context("Attachment group audio file is absent")?
            .to_owned();
        node["file"] = json!(format!("audio-assets/{file}"));
    }
    section["installable"] = json!(true);
    Ok(section)
}

/// The pin the build checks before and after loading an attachment group. It must match
/// `d2_mot::native::attachment::fingerprint`, which the build runs: the manifest's length and
/// bytes, then each distinct `particles` and `attachments` payload in path order.
fn fingerprint(directory: &Path) -> Result<String> {
    let bytes = fs::read(directory.join("asset-graph.json"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut digest = Sha256::new();
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(&bytes);
    let mut files = BTreeSet::new();
    for section in ["particles", "attachments"] {
        for node in manifest[section]["nodes"].as_array().into_iter().flatten() {
            files.insert(PathBuf::from(
                node["file"]
                    .as_str()
                    .context("Attachment group payload path")?,
            ));
        }
    }
    for name in files {
        let bytes = fs::read(directory.join(name))?;
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(hex::encode(digest.finalize()))
}
