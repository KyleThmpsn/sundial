//! Halo: Reach MCC U13 import source and native Shadowkeep translation.
//! Cache identities, material roles and source-space rigs remain explicit.
mod animation;
mod audio;
mod behavior;
mod cache;
mod gltf;
mod material;
mod model;
mod native;
mod native_marker;
mod native_material;
mod object;
mod projectile;
mod resource;
mod rig;

pub use animation::first_person::Options as FirstPersonOptions;
pub use animation::native::Binding as AnimationBinding;
pub use audio::Options as AudioOptions;
pub use native::{ImportRequest, Optic, Sights, prepare};

use anyhow::{Context, Result, ensure};
pub use cache::Tag;
pub use object::Object;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub const REVISION: u32 = 3;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    World,
    FirstPersonSpartan,
    FirstPersonElite,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    pub cache: String,
    pub group: String,
    pub path: String,
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub permutations: BTreeMap<String, String>,
    #[serde(default)]
    pub audio: Option<AudioOptions>,
}

#[derive(Clone, Debug, Serialize)]
struct Parent {
    model: usize,
    bone: Option<usize>,
    transform: [f32; 16],
}
#[derive(Clone, Debug, Serialize)]
struct PlacedModel {
    model: model::Model,
    animation: Option<Tag>,
    parent: Option<Parent>,
}
#[derive(Clone, Debug, Serialize)]
struct Scene {
    object: Object,
    models: Vec<PlacedModel>,
    limits: Vec<String>,
}

fn source_path(maps: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        name.ends_with(".map")
            && Path::new(name).components().count() == 1
            && !name.contains(['/', '\\', ':']),
        "Expected a cache filename within the configured maps directory"
    );
    let maps = maps.canonicalize().context("Reach maps directory")?;
    let path = maps
        .join(name)
        .canonicalize()
        .with_context(|| format!("Reach cache {name}"))?;
    ensure!(
        path.starts_with(&maps),
        "Cache resolves outside maps directory"
    );
    Ok(path)
}

/// Catalog every weapon and vehicle definition without collapsing map-local data.
/// Resource-only maps are retained as explicit corpus entries.
pub fn catalog(maps: &Path) -> Result<Value> {
    let mut paths = fs::read_dir(maps)?
        .map(|r| r.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("map")));
    paths.sort();
    ensure!(!paths.is_empty(), "Required Reach cache corpus is empty");
    let mut caches = Vec::new();
    let mut objects = Vec::new();
    for path in paths {
        crate::cancellation::check()?;
        let cache = cache::Cache::open(&path)?;
        let name = path
            .file_name()
            .context("Cache filename")?
            .to_str()
            .context("UTF-8 cache filename")?;
        let mut classes = BTreeMap::<String, usize>::new();
        for tag in cache.tags.iter().flatten() {
            *classes.entry(tag.group.clone()).or_default() += 1;
            if ["weap", "vehi"].contains(&tag.group.as_str()) {
                let object = Object::read(&cache, tag)
                    .with_context(|| format!("Reading {} in {name}", tag.path))?;
                let classification = if tag.group == "vehi" {
                    "vehicle"
                } else if object.first_person.iter().any(|b| b.model.is_some()) {
                    "handheld"
                } else {
                    "weapon_without_first_person_model"
                };
                objects.push(json!({"cache":name,"classification":classification,"object":object}));
            }
        }
        caches.push(json!({"cache":name,"build":cache.build,"file":resource::fingerprint(&path)?,"classes":classes,
            "exclusion":if cache.tags.is_empty() {Some("Resource-only cache with no tag directory")} else {None}}));
    }
    ensure!(!objects.is_empty(), "Required Reach object corpus is empty");
    Ok(
        json!({"source":"halo_reach","profile":"MCC PC U13","revision":REVISION,"caches":caches,"objects":objects,
        "identity_rule":"Cache filename and full datum identify an object. Shared paths do not establish equal bytes."}),
    )
}

fn selected_variant<'a>(
    object: &'a Object,
    requested: Option<&str>,
) -> Result<Option<&'a object::Variant>> {
    if requested.is_none() {
        return Ok(object
            .variants
            .iter()
            .find(|v| v.name == object.default_variant)
            .or_else(|| object.variants.iter().find(|v| v.name == "default"))
            .or_else(|| object.variants.first()));
    }
    let name = requested.unwrap_or(&object.default_variant);
    if name.is_empty() {
        return Ok(object.variants.first());
    }
    Ok(Some(
        object
            .variants
            .iter()
            .find(|v| v.name == name)
            .with_context(|| format!("Missing object variant {name} in {}", object.tag.path))?,
    ))
}

fn choices(
    object: &Object,
    variant: Option<&str>,
    overrides: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    if let Some(variant) = selected_variant(object, variant)? {
        for (region, permutations) in &variant.regions {
            if let Some(p) = permutations
                .iter()
                .filter(|p| p.probability > 0.)
                .max_by(|a, b| {
                    a.probability
                        .total_cmp(&b.probability)
                        .then_with(|| (a.runtime_index >= 0).cmp(&(b.runtime_index >= 0)))
                })
                .or(permutations.first())
            {
                result.insert(
                    region.clone(),
                    if p.runtime_index < 0 {
                        String::new()
                    } else {
                        p.name.clone()
                    },
                );
            }
        }
    }
    result.extend(overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
    Ok(result)
}

fn scene(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    request: &ExportRequest,
) -> Result<Scene> {
    let tag = cache.find(&request.group, &request.path)?;
    let object = Object::read(cache, tag)?;
    let branch = match request.view {
        View::World => object.world.as_ref().context("Object has no world model")?,
        View::FirstPersonSpartan => object
            .first_person
            .first()
            .context("Object has no Spartan first-person model")?,
        View::FirstPersonElite => object
            .first_person
            .get(1)
            .context("Object has no Elite first-person model")?,
    };
    let tag = branch
        .model
        .as_ref()
        .context("Selected object branch has no render model")?;
    let selection = choices(&object, request.variant.as_deref(), &request.permutations)?;
    let model = model::read(cache, pages, tag, &selection)?;
    let mut result=Scene {object:object.clone(),models:vec![PlacedModel {model,animation:branch.animation.clone(),parent:None}],limits:vec![
        "Source material roles are approximated by portable surface shading. Dynamic effects, shader functions and gameplay are separate from this export.".into(),
    ]};
    if request.variant.is_none()
        && !object.default_variant.is_empty()
        && !object
            .variants
            .iter()
            .any(|v| v.name == object.default_variant)
    {
        result.limits.push(format!("Declared source variant {} is absent. The declared default variant or first available variant is used, or model defaults when the variant table is empty.",object.default_variant));
    }
    let mut ancestry = BTreeSet::from([object.tag.datum]);
    children(
        cache,
        pages,
        &object,
        request.variant.as_deref(),
        0,
        &mut ancestry,
        &mut result,
    )?;
    Ok(result)
}

fn marker(model: &model::Model, name: &str) -> Result<Option<(Option<usize>, [f32; 16])>> {
    if name.is_empty() {
        return Ok(Some((None, rig::IDENTITY)));
    }
    let matches = model
        .markers
        .iter()
        .filter(|m| m.name == name)
        .filter(|m| {
            let Some(region) = m.region else { return true };
            let Some(region) = model.regions.get(region) else {
                return false;
            };
            m.permutation.is_none_or(|p| {
                region
                    .permutations
                    .get(p)
                    .is_some_and(|p| model.selected.get(&region.name) == Some(&p.name))
            })
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() <= 1,
        "Ambiguous selected attachment marker {name}"
    );
    Ok(matches
        .first()
        .map(|m| (m.bone, rig::transform(m.translation, m.rotation))))
}

fn children(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    object: &Object,
    variant: Option<&str>,
    parent_model: usize,
    ancestry: &mut BTreeSet<u32>,
    scene: &mut Scene,
) -> Result<()> {
    ensure!(
        ancestry.len() <= 32,
        "Source attachment depth exceeds limit"
    );
    let Some(variant) = selected_variant(object, variant)? else {
        return Ok(());
    };
    for child in &variant.children {
        let Some(tag) = &child.object else { continue };
        ensure!(
            ancestry.insert(tag.datum),
            "Cyclic source object attachment"
        );
        let object = Object::read(cache, tag)?;
        let tag = object
            .world
            .as_ref()
            .and_then(|b| b.model.as_ref())
            .context("Attached object has no render model")?;
        let variant = (!child.variant.is_empty()).then_some(child.variant.as_str());
        let selection = choices(&object, variant, &BTreeMap::new())?;
        let model = model::read(cache, pages, tag, &selection)?;
        let (parent_bone, parent_transform) =
            marker(&scene.models[parent_model].model, &child.parent_marker)?
                .context("Missing parent attachment marker")?;
        let (child_bone, mut child_transform) =
            marker(&model, &child.child_marker)?.context("Missing child attachment marker")?;
        if let Some(bone) = child_bone {
            child_transform = rig::multiply(&rig::world(&model.bones)?[bone], &child_transform);
        }
        let transform = rig::multiply(&parent_transform, &rig::inverse_rigid(&child_transform));
        let index = scene.models.len();
        scene.models.push(PlacedModel {
            model,
            animation: object
                .world
                .as_ref()
                .and_then(|branch| branch.animation.clone()),
            parent: Some(Parent {
                model: parent_model,
                bone: parent_bone,
                transform,
            }),
        });
        children(cache, pages, &object, variant, index, ancestry, scene)?;
        ancestry.remove(&object.tag.datum);
    }
    Ok(())
}

/// Extract a selected object to a fresh, portable rigged glTF and a source receipt.
/// The output is outside the source installation and does not alter source caches.
pub fn export(maps: &Path, request: &ExportRequest, output: &Path) -> Result<Value> {
    let maps = maps.canonicalize()?;
    let output = crate::io::outside(output, maps.parent().context("Reach source parent")?)?;
    ensure!(!output.exists(), "Reach output already exists");
    let path = source_path(&maps, &request.cache)?;
    let mut pages = resource::Pages::default();
    pages.record(&path)?;
    let cache = cache::Cache::open(&path)?;
    let scene = scene(&cache, &mut pages, request)?;
    let parent = output.parent().context("Reach output parent")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix("reach-export-")
        .tempdir_in(parent)?;
    let material_report = gltf::write(&cache, &mut pages, &scene, temporary.path())?;
    let behavior = behavior::read(&cache, &scene)?;
    projectile::export(&cache, &mut pages, &behavior, temporary.path())?;
    animation::export(&cache, &mut pages, &behavior, &scene, temporary.path())?;
    fs::write(
        temporary.path().join("behavior.json"),
        serde_json::to_vec_pretty(&behavior)?,
    )?;
    let audio = request
        .audio
        .as_ref()
        .map(|options| {
            audio::export(
                options,
                &behavior,
                &mut pages,
                &temporary.path().join("audio"),
            )
        })
        .transpose()?;
    fs::write(
        temporary.path().join("source.json"),
        serde_json::to_vec_pretty(&scene)?,
    )?;
    let report = json!({"source":"halo_reach","profile":"MCC PC U13","source_build":cache.build,"converter_revision":REVISION,
        "request":request,"inputs":pages.inputs,"resources":pages.resources,"materials":material_report,"audio":audio,
        "models":scene.models.len(),"triangles":scene.models.iter().flat_map(|m|&m.model.primitives).map(|p|p.indices.len()/3).sum::<usize>(),
        "limits":scene.limits});
    fs::write(
        temporary.path().join("export.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    let mut artifacts = BTreeMap::new();
    for entry in fs::read_dir(temporary.path())? {
        let path = entry?.path();
        if path.is_file() {
            artifacts.insert(
                path.file_name().unwrap().to_string_lossy().into_owned(),
                resource::fingerprint(&path)?,
            );
        }
    }
    fs::write(
        temporary.path().join("artifacts.json"),
        serde_json::to_vec_pretty(&artifacts)?,
    )?;
    pages.verify_inputs()?;
    ensure!(!output.exists(), "Reach output was created concurrently");
    fs::rename(temporary.path(), &output)?;
    Ok(report)
}
