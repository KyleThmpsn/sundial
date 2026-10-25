//! Build-scoped importer ownership and installed-content reuse. `package_profile::Profile`
//! serializes builds and clears this state when its turn ends, including failed builds.
#![cfg_attr(not(feature = "d2-model-importer"), allow(dead_code, unused_mut))]
use super::*;
use crate::{NewTagReference, NewTagReferenceOverride, NewTagSpec, NewTagStorageMode};
use std::{
    fs,
    sync::{Mutex, MutexGuard},
};
use sundial::package_authoring::mod_content::{ImportContent, Importer};
use tiger_pkg::{Package, PackageD2PreBL, TagHash};

/// Bump when the linked entry representation or package writer's byte contract changes.
const FORMAT: u32 = 2;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    format: u32,
    groups: BTreeMap<String, BTreeMap<String, u32>>,
    packages: BTreeMap<u16, PackageState>,
    // Preserve the serialized map bytes inside the receipt. Parsing floating-point placement
    // metadata into a JSON value again can change its spelling and invalidate its fingerprint.
    maps: BTreeMap<String, String>,
    map_packages: BTreeMap<String, Vec<u16>>,
    dependencies: Vec<ArtifactMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PackageState {
    name: String,
    entries: Vec<Entry>,
    references: Vec<Reference>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    template: u32,
    storage: NewTagStorageMode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    ordinal: usize,
    mode: ReferenceMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
enum ReferenceMode {
    Template,
    Ordinal(usize),
    Tag(u32),
}

impl From<&NewTagReferenceOverride> for Reference {
    fn from(reference: &NewTagReferenceOverride) -> Self {
        Self {
            ordinal: reference.new_tag_ordinal,
            mode: match reference.reference {
                NewTagReference::Template => ReferenceMode::Template,
                NewTagReference::Appended(ordinal) => ReferenceMode::Ordinal(ordinal),
                NewTagReference::Tag(tag) => ReferenceMode::Tag(tag.0),
            },
        }
    }
}

impl Reference {
    fn native(&self) -> NewTagReferenceOverride {
        NewTagReferenceOverride {
            new_tag_ordinal: self.ordinal,
            reference: match self.mode {
                ReferenceMode::Template => NewTagReference::Template,
                ReferenceMode::Ordinal(ordinal) => NewTagReference::Appended(ordinal),
                ReferenceMode::Tag(tag) => NewTagReference::Tag(TagHash(tag)),
            },
        }
    }
}

struct Import {
    inputs: String,
    source_inputs: Vec<ArtifactMetadata>,
    allocation: Vec<u16>,
    used: BTreeSet<u16>,
    installed: Option<(PathBuf, Member)>,
    kept: bool,
    state: State,
    conversions: usize,
}

#[derive(Default)]
struct Session {
    imports: BTreeMap<Importer, Import>,
    reserved: BTreeSet<u16>,
    native: BTreeSet<u16>,
    /// The packages that hold the build's private runtime copies, in order.
    private: Vec<u16>,
    #[cfg(feature = "d2-model-importer")]
    maps: Vec<(crate::map::MapRecipe, serde_json::Value)>,
    /// Controllers are generated after activity tags have their final addresses.
    mission_scripts: BTreeMap<String, String>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
fn session() -> MutexGuard<'static, Option<Session>> {
    SESSION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(crate) fn clear() {
    *session() = None;
}

pub(crate) fn mission_script(name: &str, source: String) -> Result<(), String> {
    let mut guard = session();
    let session = guard
        .as_mut()
        .ok_or("Mission controller has no active build")?;
    if session
        .mission_scripts
        .insert(name.to_owned(), source)
        .is_some()
    {
        return Err(format!(
            "Mission controller {name} was generated more than once"
        ));
    }
    Ok(())
}

pub(crate) fn mission_scripts() -> BTreeMap<String, String> {
    session()
        .as_ref()
        .map(|s| s.mission_scripts.clone())
        .unwrap_or_default()
}
pub(crate) fn reserved(id: u16) -> bool {
    session()
        .as_ref()
        .is_some_and(|session| session.reserved.contains(&id))
}
pub(crate) fn native(id: u16) {
    if let Some(session) = session().as_mut() {
        session.native.insert(id);
    }
}
pub(crate) fn owner(id: u16) -> Option<Importer> {
    session()
        .as_ref()?
        .imports
        .iter()
        .find(|(_, import)| import.used.contains(&id))
        .map(|(owner, _)| *owner)
}
pub(crate) fn is_kept(owner: Importer) -> bool {
    session()
        .as_ref()
        .and_then(|s| s.imports.get(&owner))
        .is_some_and(|i| i.kept)
}

fn hash(value: &impl Serialize) -> Result<String, String> {
    Ok(
        crate::artifact::digest_bytes(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
            .sha256,
    )
}

fn fingerprint(inputs: &str, allocation: &[u16], state: &State) -> Result<String, String> {
    hash(&(FORMAT, inputs, allocation, state))
}

/// The origin of a complete graph. A graph retaining native parts or borrowing installed shader
/// textures remains native, as does an explicitly mixed graph.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn graph_origin(
    graph: &serde_json::Value,
    spec: Option<&crate::WeaponCloneSpec>,
) -> Option<Importer> {
    let owner = match graph["source"].as_str() {
        Some(source) => Importer::from_source(source)?,
        // The D2 shader translator predates an explicit source marker. This recorded adapter
        // belongs to that converter. Graphs without either provenance stay in the main mod.
        None if graph["source_shader_adapter"]["source_shader_equations_retained"] == true
            && graph["source_models"].is_array() =>
        {
            Importer::D2Mot
        }
        None => return None,
    };
    if graph["kept_parts"]
        .as_array()
        .is_some_and(|parts| !parts.is_empty())
        || graph["nodes"].as_array()?.iter().any(|node| {
            node["native"] == true
                || node["source"]
                    .as_str()
                    .is_some_and(|source| source != owner.source())
        })
        || spec.is_some_and(|spec| {
            spec.overrides.dye_texture_edits.iter().any(|edit| {
                [edit.detail, edit.normal]
                    .into_iter()
                    .flatten()
                    .any(|tag| crate::shader::texture_symbol(tag).is_none())
            })
        })
    {
        return None;
    }
    Some(owner)
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn group_key(folder: &Path, spec: Option<&crate::WeaponCloneSpec>) -> String {
    format!(
        "{}:{}",
        folder.display(),
        spec.map_or("", |spec| spec.namespace.as_str())
    )
}

/// Installs immutable input identities before any native tag is allocated. Pins are checked by
/// the ordinary graph validator before and after linking. File hashes, rather than timestamps,
/// decide whether map inputs and installed packages can be reused.
pub(crate) fn begin(
    target: &Path,
    source: &Path,
    weapons: &[crate::WeaponCloneSpec],
    maps: &[crate::map::MapRecipe],
    stock: &[ArtifactMetadata],
) -> Result<(), String> {
    if !crate::package_profile::host().is_standalone() {
        return Ok(());
    }
    let mut inputs: BTreeMap<Importer, Vec<serde_json::Value>> = BTreeMap::new();
    let mut templates = BTreeMap::<Importer, BTreeSet<u16>>::new();
    #[cfg(feature = "d2-model-importer")]
    let mut map_records = Vec::new();
    #[cfg(feature = "d2-model-importer")]
    {
        for spec in weapons {
            let Some(reference) = &spec.overrides.imported_graph else {
                continue;
            };
            for (graph, binding) in std::iter::once((reference, Some(spec))).chain(
                reference.attachments.iter().map(|attachment| {
                    (
                        attachment,
                        (spec.kind == crate::ItemKind::Armor).then_some(spec),
                    )
                }),
            ) {
                let description: serde_json::Value = serde_json::from_slice(
                    &fs::read(graph.directory.join("asset-graph.json"))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                let Some(origin) = graph_origin(&description, binding) else {
                    continue;
                };
                let ids = templates.entry(origin).or_default();
                for node in description["nodes"].as_array().into_iter().flatten() {
                    for tag in [node.get("template"), node.get("source_parent")]
                        .into_iter()
                        .flatten()
                        .chain(node["inherited"].as_array().into_iter().flatten())
                        .filter_map(serde_json::Value::as_u64)
                    {
                        ids.insert(TagHash(tag as u32).pkg_id());
                    }
                }
                inputs.entry(origin).or_default().push(serde_json::json!({
                    "key": group_key(&graph.directory, binding),
                    "pin": {
                        "directory": graph.directory,
                        "sha256": graph.sha256,
                        "converter_revision": graph.converter_revision,
                        "material_inputs": graph.material_inputs,
                    },
                    "markers": binding.map(|spec| &spec.overrides.marker_offsets),
                    "dyes": binding.map(|spec| &spec.overrides.dye_edits),
                    "textures": binding.map(|spec| &spec.overrides.dye_texture_edits),
                    "rows": binding.and_then(|spec| spec.overrides.render_dye_rows.as_ref())
                        .map(|rows| rows.iter().map(|row| row.iter().map(|entry|
                            (entry.channel_index, entry.dye_reference_index)
                        ).collect::<Vec<_>>()).collect::<Vec<_>>()),
                    "namespace": binding.map(|spec| &spec.namespace),
                    "identity": binding.map(|spec| spec.identity.item_hash),
                }));
            }
        }
        for map in maps {
            if let Some(source) = &map.source {
                let input = map_inputs(map, source)?;
                inputs
                    .entry(Importer::HaloReach)
                    .or_default()
                    .push(input.clone());
                map_records.push((map.clone(), input));
            }
        }
    }
    #[cfg(not(feature = "d2-model-importer"))]
    let _ = (weapons, maps);
    if inputs.is_empty() {
        // The private runtime's packages still take their ids from the session.
        *session() = Some(Session::default());
        return Ok(());
    }
    let install = target.parent().ok_or("Missing game root")?;
    mod_content::files(install)?;
    let directories = mod_content::family_directories(install)?;
    let mut current = Session::default();
    #[cfg(feature = "d2-model-importer")]
    {
        current.maps = map_records;
    }
    for (origin, mut items) in inputs {
        items.sort_by_cached_key(|item| item.to_string());
        let source_inputs = source_dependencies(
            source,
            &templates.remove(&origin).unwrap_or_default(),
            &mut BTreeMap::new(),
        )?;
        #[cfg(feature = "d2-model-importer")]
        let converter = parhelion_import::CONVERTER_REVISION;
        #[cfg(not(feature = "d2-model-importer"))]
        let converter = 0;
        let inputs = hash(&(
            FORMAT,
            env!("CARGO_PKG_VERSION"),
            converter,
            stock,
            &source_inputs,
            items,
        ))?;
        let installed = directories
            .get(&origin.id())
            .map(|directory| {
                let manifest: Manifest =
                    mod_content::read_json(&directory.join(mod_content::MANIFEST))?;
                let receipt: Receipt =
                    mod_content::read_json(&directory.join(mod_content::RECEIPT))?;
                manifest.validate()?;
                receipt.validate()?;
                Ok::<_, String>((
                    directory.clone(),
                    Member {
                        manifest,
                        receipt,
                        kept: false,
                    },
                ))
            })
            .transpose()?;
        let mut allocation = Vec::new();
        let mut state = State {
            format: FORMAT,
            ..Default::default()
        };
        let mut kept = false;
        if let Some((directory, member)) = &installed {
            for artifact in &member.receipt.artifacts {
                let digest = crate::artifact::digest_file(&directory.join(&artifact.path))
                    .map_err(|e| e.to_string())?;
                if digest.byte_length != artifact.byte_length
                    || !digest.sha256.eq_ignore_ascii_case(&artifact.sha256)
                {
                    return Err(format!(
                        "{} changed outside Parhelion. Its files were preserved",
                        member.manifest.name
                    ));
                }
            }
            if let Some(content) = &member.receipt.content {
                allocation = content.allocation.clone();
                if let Ok(previous) = serde_json::from_value::<State>(content.state.clone()) {
                    kept = previous.format == FORMAT
                        && inputs == content.inputs
                        && fingerprint(&inputs, &allocation, &previous)? == content.fingerprint
                        && member.manifest == Manifest::for_importer(origin)
                        && dependencies_match(target, &previous.dependencies)?;
                    if kept {
                        state = previous;
                    }
                }
            }
            if allocation.is_empty() {
                allocation = member
                    .receipt
                    .artifacts
                    .iter()
                    .filter_map(|artifact| {
                        artifact
                            .path
                            .ends_with(".pkg")
                            .then(|| package_id(&artifact.path))
                            .flatten()
                    })
                    .collect();
            }
        }
        current.reserved.extend(&allocation);
        current.imports.insert(
            origin,
            Import {
                inputs,
                source_inputs,
                allocation,
                used: BTreeSet::new(),
                installed,
                kept,
                state,
                conversions: 0,
            },
        );
    }
    let window = crate::package_profile::window();
    if current.reserved.iter().any(|id| {
        !(window.base..=window.top).contains(id) || *id == crate::package_profile::host().id
    }) {
        return Err("An installed importer ID is outside the free package window. Resolve the conflicting installed content before rebuilding".into());
    }
    *session() = Some(current);
    Ok(())
}

fn dependencies_match(target: &Path, dependencies: &[ArtifactMetadata]) -> Result<bool, String> {
    for expected in dependencies {
        if Path::new(&expected.file_name).components().count() != 1 {
            return Err("Invalid native dependency path in importer receipt".into());
        }
        let path = target.join(&expected.file_name);
        mod_content::checked_path(target, &path)?;
        let actual = crate::artifact::digest_file(&path).map_err(|e| {
            format!(
                "Could not verify native importer dependency {}: {e}",
                path.display()
            )
        })?;
        if actual.byte_length != expected.byte_length || actual.sha256 != expected.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Every template's stock package generations participate in the importer identity. This also
/// covers cloned native loading companions and source material envelopes, which are absent
/// from the investment-only source-artifact list.
pub(crate) fn finish_sources(source: &Path) -> Result<(), String> {
    let mut guard = session();
    let Some(current) = guard.as_mut() else {
        return Ok(());
    };
    let mut digests = BTreeMap::new();
    for import in current.imports.values_mut() {
        if !dependencies_match(source, &import.source_inputs)? {
            return Err("An importer's native inputs changed during the build".into());
        }
        if import.kept {
            if !dependencies_match(source, &import.state.dependencies)? {
                return Err("A kept importer's native templates changed during the build".into());
            }
            continue;
        }
        let ids = import
            .state
            .packages
            .values()
            .flat_map(|p| &p.entries)
            .map(|entry| TagHash(entry.template).pkg_id())
            .collect::<BTreeSet<_>>();
        import.state.dependencies = source_dependencies(source, &ids, &mut digests)?;
    }
    Ok(())
}

fn source_dependencies(
    source: &Path,
    ids: &BTreeSet<u16>,
    digests: &mut BTreeMap<String, ArtifactMetadata>,
) -> Result<Vec<ArtifactMetadata>, String> {
    let mut dependencies = Vec::new();
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.extension().is_some_and(|extension| extension == "pkg")
            || !package_id(path.to_str().ok_or("Native dependency path is not UTF-8")?)
                .is_some_and(|id| ids.contains(&id))
        {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Native dependency name is not UTF-8")?;
        let metadata = if let Some(metadata) = digests.get(&name) {
            metadata.clone()
        } else {
            let digest = crate::artifact::digest_file(&path).map_err(|e| e.to_string())?;
            let metadata = ArtifactMetadata {
                file_name: name.clone(),
                byte_length: digest.byte_length,
                sha256: digest.sha256,
            };
            digests.insert(name, metadata.clone());
            metadata
        };
        dependencies.push(metadata);
    }
    dependencies.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(dependencies)
}

#[cfg(feature = "d2-model-importer")]
fn map_inputs(
    map: &crate::map::MapRecipe,
    source: &crate::map::MapSource,
) -> Result<serde_json::Value, String> {
    let mut paths = parhelion_import::halo_reach::level::source_files(&source.maps, &source.map)
        .map_err(|e| format!("Map {} inputs: {e:#}", map.name))?;
    paths.extend(source.variant.iter().cloned());
    paths.extend(source.mission_bindings.iter().cloned());
    let decoder = source
        .audio_decoder
        .clone()
        .or_else(|| parhelion_import::tools::located(parhelion_import::tools::Tool::Vgmstream));
    if let Some(decoder) = &decoder {
        paths.push(decoder.clone());
        paths.push(source.maps.join("../fmod/pc/sfx.fsb"));
        {
            if source.audio_banks.is_empty() {
                paths.push(source.maps.join("../fmod/pc/english.fsb"));
            } else {
                paths.extend(source.audio_banks.iter().cloned());
            }
        }
    }
    let info = paths
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "fsb"))
        .map(|path| path.with_extension("fsb.info"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.extend(info);
    paths.sort();
    paths.dedup();
    let files = paths
        .into_iter()
        .map(|path| {
            crate::artifact::digest_file(&path)
                .map(|digest| (path.clone(), digest))
                .map_err(|e| format!("Could not hash map input {}: {e}", path.display()))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(serde_json::json!({
        "map": map.name, "donor": map.donor, "source": source, "decoder": decoder, "files": files,
        "reach_revision": parhelion_import::halo_reach::REVISION,
        // A campaign's areas share one conversion, so kept geometry from per-area builds differs.
        "areas": "shared",
    }))
}

/// Reuse the build's checked file identities across its areas, whose zone sets and offsets differ.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn map_source_identity(source: &crate::map::MapSource) -> Option<serde_json::Value> {
    let guard = session();
    guard.as_ref()?.maps.iter().find_map(|(map, input)| {
        let original = map.source.as_ref()?;
        (original.maps == source.maps
            && original.map == source.map
            && original.variant == source.variant
            && (source.mission_bindings.is_none()
                || original.mission_bindings == source.mission_bindings)
            && original.audio_decoder == source.audio_decoder
            && original.audio_banks == source.audio_banks)
            .then(|| input["files"].clone())
    })
}

pub(crate) fn verify_inputs() -> Result<(), String> {
    #[cfg(feature = "d2-model-importer")]
    {
        let maps = session()
            .as_ref()
            .map(|s| s.maps.clone())
            .unwrap_or_default();
        for (map, input) in maps {
            if map_inputs(&map, map.source.as_ref().unwrap())? != input {
                return Err(format!("Map {} inputs changed during the build", map.name));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn map_state(name: &str) -> Result<Option<serde_json::Value>, String> {
    let guard = session();
    let Some(import) = guard
        .as_ref()
        .and_then(|session| session.imports.get(&Importer::HaloReach))
        .filter(|import| import.kept)
    else {
        return Ok(None);
    };
    import
        .state
        .maps
        .get(name)
        .map(|text| serde_json::from_str(text).map_err(|e| e.to_string()))
        .transpose()
}

#[cfg(feature = "d2-model-importer")]
pub(crate) fn remember_map(name: &str, mut value: serde_json::Value) -> Result<(), String> {
    if let Some(import) = session()
        .as_mut()
        .and_then(|s| s.imports.get_mut(&Importer::HaloReach))
        && !import.kept
    {
        // Reused resources come from installed packages. Their ordered symbols, typed fixups and
        // placement metadata rebuild the native world chain. Conversion reports and
        // source audio bytes would duplicate the cache and grow receipts with every map.
        if let Some(nodes) = value["nodes"].as_array_mut() {
            for node in nodes {
                *node = serde_json::json!({
                    "symbol": node["symbol"], "patches": node["patches"],
                    "reference": node["reference"], "texture_pair": node["texture_pair"],
                });
            }
        }
        value["report"] = serde_json::Value::Null;
        import.conversions += 1;
        import.state.maps.insert(
            name.into(),
            serde_json::to_string(&value).map_err(|e| e.to_string())?,
        );
    }
    Ok(())
}

fn package_id(name: &str) -> Option<u16> {
    let parsed = tiger_pkg::manager::PackagePath::parse(name)?;
    u16::from_str_radix(&parsed.id, 16).ok()
}

pub(crate) fn allocate(owner: Importer, index: usize) -> Result<u16, String> {
    let mut guard = session();
    let current = guard
        .as_mut()
        .ok_or("Importer allocation has no build session")?;
    let import = current
        .imports
        .get_mut(&owner)
        .ok_or("Importer was not declared before allocation")?;
    if let Some(&id) = import.allocation.get(index) {
        return Ok(id);
    }
    let window = crate::package_profile::window();
    let ids = (window.base..=window.top).rev();
    let id = ids
        .into_iter()
        .find(|id| !current.reserved.contains(id) && !current.native.contains(id))
        .ok_or("No free package ID remains for imported content")?;
    import.allocation.push(id);
    current.reserved.insert(id);
    Ok(id)
}

/// The package that holds the build's private runtime copies `index` packages past their first.
/// Each takes the highest id of the window that no package of the build or an importer holds, as
/// importer packages do, and asset and map packages count past it. Every package a build adds
/// then sits at the top of the window, clear of the ids Sunrise Studio gives a mod's new packages
/// from `0x0AA0` up without reading other mods.
pub(crate) fn private_runtime(index: usize) -> Result<u16, String> {
    let mut guard = session();
    let current = guard
        .as_mut()
        .ok_or("Private runtime allocation has no build session")?;
    let window = crate::package_profile::window();
    let host = crate::package_profile::host().id;
    while current.private.len() <= index {
        let id = (window.base..=window.top)
            .rev()
            .find(|id| {
                *id != host && !current.reserved.contains(id) && !current.native.contains(id)
            })
            .ok_or("No free package ID remains for the build's private runtime")?;
        current.private.push(id);
        current.reserved.insert(id);
    }
    Ok(current.private[index])
}

pub(crate) fn use_id(owner: Importer, id: u16) {
    if let Some(import) = session().as_mut().and_then(|s| s.imports.get_mut(&owner)) {
        import.used.insert(id);
    }
}

pub(crate) fn map_packages(name: &str, count: usize) -> Result<Vec<u16>, String> {
    let owner = Importer::HaloReach;
    let kept = {
        let guard = session();
        let import = guard
            .as_ref()
            .and_then(|s| s.imports.get(&owner))
            .ok_or("Map allocation has no importer session")?;
        if import.kept {
            Some(
                import
                    .state
                    .map_packages
                    .get(name)
                    .cloned()
                    .filter(|ids| ids.len() == count)
                    .ok_or("Kept map allocation differs from its resource count")?,
            )
        } else {
            None
        }
    };
    if let Some(ids) = kept {
        for &id in &ids {
            use_id(owner, id);
        }
        return Ok(ids);
    }
    let start = session()
        .as_ref()
        .and_then(|s| s.imports.get(&owner))
        .ok_or("Importer allocation has no build session")?
        .used
        .len();
    let mut ids = Vec::new();
    for offset in 0..count {
        let id = allocate(owner, start + offset)?;
        use_id(owner, id);
        ids.push(id);
    }
    session()
        .as_mut()
        .unwrap()
        .imports
        .get_mut(&owner)
        .unwrap()
        .state
        .map_packages
        .insert(name.into(), ids.clone());
    Ok(ids)
}

pub(crate) fn record_package(
    id: u16,
    name: &str,
    tags: &[NewTagSpec],
    references: &[NewTagReferenceOverride],
) {
    let mut guard = session();
    let Some(import) = guard
        .as_mut()
        .and_then(|session| session.imports.values_mut().find(|i| i.used.contains(&id)))
    else {
        return;
    };
    if import.kept {
        return;
    }
    import.state.packages.insert(
        id,
        PackageState {
            name: name.into(),
            entries: tags
                .iter()
                .map(|tag| Entry {
                    template: tag.template_tag.0,
                    storage: tag.storage,
                })
                .collect(),
            references: references.iter().map(Reference::from).collect(),
        },
    );
}

pub(crate) fn remember_group(owner: Importer, key: String, symbols: &BTreeMap<String, TagHash>) {
    if let Some(import) = session().as_mut().and_then(|s| s.imports.get_mut(&owner)) {
        import.conversions += 1;
        import.state.groups.insert(
            key,
            symbols
                .iter()
                .map(|(name, tag)| (name.clone(), tag.0))
                .collect(),
        );
    }
}

pub(crate) fn group(owner: Importer, key: &str) -> Option<BTreeMap<String, TagHash>> {
    let guard = session();
    let import = guard.as_ref()?.imports.get(&owner)?;
    if !import.kept {
        return None;
    }
    Some(
        import
            .state
            .groups
            .get(key)?
            .iter()
            .map(|(name, tag)| (name.clone(), TagHash(*tag)))
            .collect(),
    )
}

/// Rehydrate linked entries for loading-index validation and main-mod references. The output
/// package job reads the verified installed bytes, so these payloads never pass through a writer.
pub(crate) fn restore(id: u16) -> Result<crate::asset_packages::AssetPackage, String> {
    let (path, state) = {
        let guard = session();
        let import = guard
            .as_ref()
            .and_then(|s| {
                s.imports
                    .values()
                    .find(|i| i.kept && i.allocation.contains(&id))
            })
            .ok_or("No kept importer owns the requested package")?;
        let state = import
            .state
            .packages
            .get(&id)
            .ok_or("Kept package metadata is missing")?
            .clone();
        let path = import
            .installed
            .as_ref()
            .unwrap()
            .0
            .join("packages")
            .join(&state.name);
        (path, state)
    };
    let package = PackageD2PreBL::open(path.to_str().ok_or("Package path is not UTF-8")?)
        .map_err(|e| e.to_string())?;
    if package.entries().len() != state.entries.len() {
        return Err("Kept package entry count changed".into());
    }
    let tags = state
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            package
                .read_entry(index)
                .map(|payload| NewTagSpec {
                    template_tag: TagHash(entry.template),
                    payload,
                    storage: entry.storage,
                })
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(crate::asset_packages::AssetPackage {
        id,
        tags,
        references: state.references.iter().map(Reference::native).collect(),
    })
}

pub(crate) fn kept_path(name: &str) -> Option<PathBuf> {
    let guard = session();
    guard.as_ref()?.imports.values().find_map(|import| {
        if !import.kept {
            return None;
        }
        let (directory, member) = import.installed.as_ref()?;
        let artifact = member
            .receipt
            .artifacts
            .iter()
            .find(|a| a.path.rsplit('/').next() == Some(name))?;
        Some(directory.join(&artifact.path))
    })
}

pub(crate) fn conversions() -> BTreeMap<String, usize> {
    session()
        .as_ref()
        .map(|s| {
            s.imports
                .iter()
                .map(|(origin, i)| (origin.source().into(), i.conversions))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn members(
    artifacts: &[ArtifactMetadata],
    merge: &[ArtifactMetadata],
    diagnostics: Option<&ArtifactMetadata>,
    scripts: &[ArtifactMetadata],
) -> Result<Vec<Member>, String> {
    let guard = session();
    let Some(current) = guard.as_ref() else {
        return Ok(Vec::new());
    };
    let imports = current
        .imports
        .iter()
        .filter(|(_, import)| !import.used.is_empty())
        .collect::<Vec<_>>();
    if imports.is_empty() {
        return Ok(Vec::new());
    }
    let mut main = Member {
        manifest: Manifest::default(),
        receipt: Receipt {
            schema: 1,
            mod_id: mod_content::ID.into(),
            artifacts: vec![],
            content: None,
        },
        kept: false,
    };
    let mut members = Vec::new();
    for (origin, import) in &imports {
        let manifest = Manifest::for_importer(**origin);
        main.manifest.require(&manifest);
        let allocation = import
            .allocation
            .iter()
            .filter(|id| import.used.contains(id))
            .copied()
            .collect::<Vec<_>>();
        let content = ImportContent {
            fingerprint: fingerprint(&import.inputs, &allocation, &import.state)?,
            inputs: import.inputs.clone(),
            allocation,
            state: serde_json::to_value(&import.state).map_err(|e| e.to_string())?,
        };
        members.push(Member {
            manifest,
            receipt: Receipt {
                schema: 1,
                mod_id: origin.id(),
                artifacts: vec![],
                content: Some(content),
            },
            kept: import.kept,
        });
    }
    for artifact in artifacts
        .iter()
        .chain(merge)
        .chain(diagnostics)
        .chain(scripts)
    {
        let package_name = artifact
            .file_name
            .strip_suffix(".merge")
            .map(|s| format!("{s}.pkg"))
            .unwrap_or_else(|| artifact.file_name.clone());
        let profile = crate::package_profile::authored_package_for_file_name(&package_name);
        let owner = profile.and_then(|profile| {
            imports
                .iter()
                .find(|(_, i)| i.used.contains(&profile.package_id))
                .map(|(owner, _)| **owner)
        });
        let path = if let Some(path) = mod_content::mission_script::path(&artifact.file_name) {
            path
        } else if artifact.file_name == mod_content::map_diagnostics::FILE_NAME {
            artifact.file_name.clone()
        } else {
            format!(
                "{}/{}",
                if profile.is_some_and(|p| p.stock_overlay) {
                    "patches"
                } else {
                    "packages"
                },
                artifact.file_name
            )
        };
        let receipt = owner
            .and_then(|owner| members.iter_mut().find(|m| m.manifest.id == owner.id()))
            .map_or(&mut main.receipt, |m| &mut m.receipt);
        receipt.artifacts.push(mod_content::Artifact {
            path,
            byte_length: artifact.byte_length,
            sha256: artifact.sha256.clone(),
        });
    }
    main.receipt.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
    for member in &mut members {
        member.receipt.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
        member.receipt.validate()?;
        if member.kept {
            let old = &current.imports[&Importer::from_id(&member.manifest.id).unwrap()]
                .installed
                .as_ref()
                .unwrap()
                .1;
            if member.receipt != old.receipt {
                return Err("A kept importer's output differs from its installed receipt".into());
            }
        }
    }
    members.insert(0, main);
    Ok(members)
}
