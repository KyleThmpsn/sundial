//! Native carriers keep their runtime and receive private weighted Reach geometry.
use super::*;
use crate::{
    presentation::{
        Graph,
        geometry::{self, Mesh},
        put,
    },
    tiger::{
        payload::Payload,
        reader::{Reader, write_json},
    },
};
mod fragment;
mod inspection;
mod marker;
pub(super) mod material;
mod optic;
mod report;
pub(super) mod scale;
pub(super) mod seat;
mod settings;
mod vehicle;
pub use seat::DriverPose;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpticView {
    #[default]
    Model,
    Separate,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reticle {
    #[default]
    Crosshair,
    Rangefinder,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Optic {
    /// The source material whose geometry bounds define the sight aperture.
    pub aperture: String,
    /// All source lens materials that must transmit the native scene.
    pub lenses: Vec<String>,
    /// Camera distance behind the rear aperture, in native metres.
    pub eye_relief: f32,
    /// A separate view preserves the physical glass and places aiming beyond the model.
    #[serde(default)]
    pub view: OpticView,
    /// The authored overlay for a separate sight view.
    #[serde(default)]
    pub reticle: Reticle,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sights {
    /// Source model sight points in native metres, with X forward and Z up.
    pub rear: [f32; 3],
    pub front: [f32; 3],
    /// Camera distance behind the rear sight along its axis.
    pub eye_relief: f32,
    /// A separate view supports a source sight drawn as a single holographic surface.
    #[serde(default)]
    pub view: OpticView,
    #[serde(default)]
    pub reticle: Reticle,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub slug: String,
    pub source: ExportRequest,
    /// A normal Parhelion recipe supplies the donor and private authored identity.
    pub recipe: Value,
    /// Uniform adaptation of source lengths, including rigs and vehicle physics.
    #[serde(default = "scale::one")]
    pub scale: f32,
    /// Inventory display size relative to the native gear carrier.
    #[serde(default = "scale::one")]
    pub inspection_scale: f32,
    /// Explicit native vehicle entity. It must agree with the recipe's summon choice.
    #[serde(default)]
    pub runtime_entity: Option<u32>,
    /// Carrier-pose correction in the source driver marker's local frame.
    /// Source rider actions already provide their own pose and omit this correction.
    #[serde(default)]
    pub driver_pose: Option<DriverPose>,
    /// Override automatic selection of a native carrier for one modeled source projectile.
    #[serde(default)]
    pub projectile_entity: Option<u32>,
    /// Source bone name to native bone name. Unmapped nodes follow their mapped ancestor.
    #[serde(default)]
    pub bone_aliases: BTreeMap<String, String>,
    /// Explicit source clips fitted to existing native vehicle clip durations.
    #[serde(default)]
    pub animation_bindings: Vec<super::animation::native::Binding>,
    /// Source actions fitted to the native character arms without replacing their geometry.
    #[serde(default)]
    pub first_person: Option<super::animation::first_person::Options>,
    /// Source body actions selected by the private weapon or driver profile.
    #[serde(default)]
    pub character: Option<super::animation::character::Options>,
    /// Explicit appearance state for source parameters whose events are not translated.
    #[serde(default)]
    pub material_overrides: BTreeMap<String, BTreeMap<String, [f32; 4]>>,
    /// Explicit vehicle body and trim materials affected by an equipped shader.
    #[serde(default)]
    pub paint: BTreeMap<String, super::paint::Paint>,
    #[serde(default)]
    pub optic: Option<Optic>,
    /// Explicit iron sights for a source that has no lens aperture.
    #[serde(default)]
    pub sights: Option<Sights>,
    /// Optional native item supplying the sight assembly independently of firing behavior.
    #[serde(default)]
    pub optic_item: Option<u32>,
    /// Optional framing override for generated inventory artwork.
    #[serde(default)]
    pub icon: Option<crate::artwork::Settings>,
}

fn hash(value: &Value, key: &str) -> Result<u32> {
    crate::graph::hash(value, key)
}
fn name_hash(name: &str) -> u32 {
    name.bytes()
        .fold(0x811c9dc5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}

fn skeleton(native: &mut Reader, entity: u32, item: u32, palette: usize) -> Result<Value> {
    let entity = native.tag(entity, Some(0x80809c0f))?;
    let mut choices = Vec::new();
    for row in entity.array(16, 12, None)? {
        let tag = entity.u32(row)?;
        let owner = native.tag(tag, Some(0x80809c36))?;
        let resource = owner.pointer(24)?;
        let class = owner.u32(resource.checked_sub(4).context("Skeleton resource")?)?;
        if matches!(class, 0x8080853e | 0x80808546) {
            choices.push(crate::tiger::rig::decode(
                &owner, resource, class, tag, false,
            )?);
        }
    }
    ensure!(
        choices.len() <= 1,
        "Native render entity has ambiguous skeletons"
    );
    if let Some(skeleton) = choices.pop() {
        return Ok(skeleton);
    }
    // Weapon art entities borrow their bone palette from the runtime entity.
    // A missing art-local skeleton does not make a weapon rigid.
    let runtime = if item == 0 {
        json!({"skeletons":[]})
    } else {
        crate::tiger::rig::inspect(native, item, false)?
    };
    let skeletons = runtime["skeletons"]
        .as_array()
        .context("Native runtime skeletons")?;
    let mut candidates = skeletons
        .iter()
        .filter(|s| s["bones"].as_array().is_some_and(|b| b.len() == palette))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        let handle = format!("{:08X}", name_hash("b_handle"));
        candidates = skeletons
            .iter()
            .filter(|s| s["bones"][0]["name_hash"] == handle)
            .collect();
    }
    // Equipment art can use a prefix of its sole runtime skeleton. The model's
    // palette bounds referenced joints, not the complete runtime hierarchy.
    if candidates.is_empty() && skeletons.len() == 1 {
        candidates = skeletons
            .iter()
            .filter(|s| {
                s["bones"]
                    .as_array()
                    .is_some_and(|bones| palette > 1 && palette <= bones.len())
            })
            .collect();
    }
    if let Some(first) = candidates.first() {
        ensure!(
            candidates.iter().all(|s| s["bones"] == first["bones"]),
            "Native model palette matches incompatible runtime skeletons"
        );
        return Ok((*first).clone());
    }
    ensure!(
        palette <= 1,
        "Native model palette {palette} has no matching runtime skeleton. Available sizes: {:?}",
        skeletons
            .iter()
            .map(|s| s["bones"].as_array().map(Vec::len))
            .collect::<Vec<_>>()
    );
    Ok(
        json!({"bones":[{"index":0,"name_hash":format!("{:08X}",name_hash("b_handle")),"parent":-1}],"static":true,"rigid_uniforms":palette==0}),
    )
}

fn fallback(native: &Value, names: &BTreeMap<u32, u8>) -> Result<(u8, BTreeSet<u8>)> {
    let Some(index) = native["fallback_bone"].as_u64() else {
        return Ok((
            names.get(&name_hash("b_handle")).copied().unwrap_or(0),
            BTreeSet::new(),
        ));
    };
    let root = u8::try_from(index)?;
    let bones = native["bones"]
        .as_array()
        .context("Native fallback bones")?;
    let mut ancestors = BTreeSet::new();
    let mut index = usize::from(root);
    loop {
        let bone = bones
            .get(index)
            .context("Native fallback index exceeds skeleton")?;
        let parent = bone["parent"].as_i64().context("Native fallback parent")?;
        if parent < 0 {
            break;
        }
        let parent = u8::try_from(parent)?;
        ensure!(
            parent != root && ancestors.insert(parent),
            "Native fallback hierarchy is cyclic"
        );
        index = usize::from(parent);
    }
    Ok((root, ancestors))
}

fn retarget(
    model: &model::Model,
    native: &Value,
    aliases: &BTreeMap<String, String>,
) -> Result<(Vec<u8>, Vec<Value>)> {
    let bones = native["bones"].as_array().context("Native bones")?;
    ensure!(
        !bones.is_empty() && bones.len() <= 256,
        "Native skeleton exceeds weight index capacity"
    );
    if let Some(first) = native["source_bone_first"].as_u64() {
        let first = usize::try_from(first)?;
        ensure!(
            first + model.bones.len() == bones.len() && aliases.is_empty(),
            "Source animation rig cannot also use donor bone aliases"
        );
        let map = (first..bones.len())
            .map(u8::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let report = model.bones.iter().zip(&map)
            .map(|(bone,index)| json!({"source":bone.name,"native_index":index,"method":"Appended source animation control"})).collect();
        return Ok((map, report));
    }
    let mut names = BTreeMap::new();
    for (i, bone) in bones.iter().enumerate() {
        names.insert(
            u32::from_str_radix(bone["name_hash"].as_str().context("Native bone name")?, 16)?,
            i as u8,
        );
    }
    let (root, unused_ancestors) = fallback(native, &names)?;
    let mut result = vec![None; model.bones.len()];
    let mut reasons = vec![String::new(); model.bones.len()];
    for (i, bone) in model.bones.iter().enumerate() {
        let default = match bone.name.as_str() {
            "b_gun" | "b_weapon" => "b_handle",
            "b_mag" => "b_magazine",
            other => other,
        };
        let name = aliases
            .get(&bone.name)
            .map(String::as_str)
            .unwrap_or(default);
        if let Some(&index) = names.get(&name_hash(name)) {
            // An implicit name match above the drawn branch would defeat its fallback.
            // Explicit aliases retain the author's chosen native role.
            if aliases.contains_key(&bone.name) || !unused_ancestors.contains(&index) {
                result[i] = Some(index);
                reasons[i] = format!("Native role {name}");
            }
        } else if aliases.contains_key(&bone.name) {
            anyhow::bail!("Explicit native bone alias {name} is absent");
        }
    }
    for i in 0..model.bones.len() {
        if result[i].is_some() {
            continue;
        }
        let mut parent = model.bones[i].parent;
        let mut selected = None;
        while let Some(index) = parent {
            if let Some(bone) = result[index] {
                selected = Some(bone);
                break;
            }
            parent = model.bones[index].parent;
        }
        result[i] = Some(selected.unwrap_or(root));
        reasons[i] = if selected.is_some() {
            "Mapped source ancestor"
        } else {
            if native["fallback_bone"].is_number() {
                "Native drawn-bone ancestor"
            } else {
                "Native root fallback"
            }
        }
        .into();
    }
    let result = result.into_iter().map(Option::unwrap).collect::<Vec<_>>();
    let report = model
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| json!({"source":b.name,"native_index":result[i],"method":reasons[i]}))
        .collect();
    Ok((result, report))
}

fn weights(joints: [u16; 4], weights: [f32; 4], first: usize, count: usize) -> Result<[u8; 8]> {
    ensure!(
        weights.iter().all(|v| v.is_finite() && *v >= 0.)
            && (weights.iter().sum::<f32>() - 1.).abs() < 0.0001,
        "Invalid source influences"
    );
    let mut out = [0; 8];
    let mut sum = 0u32;
    let mut largest = 0;
    for (i, (joint, weight)) in joints.into_iter().zip(weights).enumerate() {
        ensure!(
            weight == 0. || usize::from(joint) < count,
            "Source joint outside palette"
        );
        out[i] = (weight * 255.).floor().clamp(0., 255.) as u8;
        out[i + 4] = if weight > 0. {
            u8::try_from(first + usize::from(joint))?
        } else {
            0
        };
        sum += u32::from(out[i]);
        if out[i] > out[largest] {
            largest = i;
        }
    }
    ensure!(sum <= 255, "Native weights exceed one");
    out[largest] += u8::try_from(255 - sum)?;
    Ok(out)
}

struct Assembly {
    mesh: Mesh,
    materials: BTreeMap<u32, crate::halo_reach::material::Material>,
    rig_report: Vec<Value>,
    palette: Vec<u8>,
    groups: Vec<u16>,
}

fn assemble(
    scene: &Scene,
    skeleton: &Value,
    aliases: &BTreeMap<String, String>,
) -> Result<Assembly> {
    let mut mesh = Mesh {
        positions: Vec::new(),
        attributes: Vec::new(),
        groups: Vec::new(),
        weights: Vec::new(),
        bones: if skeleton["rigid_uniforms"] == true {
            0
        } else {
            u32::try_from(
                skeleton["bones"]
                    .as_array()
                    .context("Native skeleton")?
                    .len(),
            )?
        },
    };
    let mut materials = BTreeMap::new();
    let mut reports = Vec::new();
    let mut palette = Vec::new();
    let mut placement = Vec::new();
    let mut groups = Vec::new();
    for (model_index, placed) in scene.models.iter().enumerate() {
        let model = &placed.model;
        let transform = if let Some(parent) = &placed.parent {
            let mut transform = placement[parent.model];
            if let Some(bone) = parent.bone {
                transform = rig::multiply(
                    &transform,
                    &rig::world(&scene.models[parent.model].model.bones)?[bone],
                );
            }
            rig::multiply(&transform, &parent.transform)
        } else {
            rig::IDENTITY
        };
        placement.push(transform);
        let (mapping, report) = retarget(model, skeleton, aliases)?;
        let first = palette.len();
        palette.extend(mapping);
        ensure!(
            palette.len() <= 256,
            "Source joint palette exceeds native byte storage"
        );
        reports.push(json!({"model":model.tag,"first_joint":first,"source_hierarchy":model.bones,"bones":report}));
        for primitive in &model.primitives {
            groups.push(if model_index == 0 {
                scene
                    .damage
                    .as_ref()
                    .map(|damage| damage.group(primitive))
                    .transpose()?
                    .unwrap_or(0)
            } else {
                0
            });
            let mat = primitive
                .material
                .and_then(|i| model.materials.get(i))
                .cloned()
                .unwrap_or(crate::halo_reach::material::Material {
                    decal: None,
                    tag: None,
                    options: BTreeMap::new(),
                    properties: Vec::new(),
                });
            let key = mat.tag.as_ref().map(|t| t.datum).unwrap_or(u32::MAX);
            materials.entry(key).or_insert(mat);
            let base = u32::try_from(mesh.positions.len())?;
            for vertex in &primitive.vertices {
                mesh.positions
                    .push(rig::point(&transform, vertex.position, true));
                let vector = |v: [f32; 3]| -> Result<[f32; 3]> {
                    let v = std::array::from_fn::<_, 3, _>(|a| {
                        transform[a] * v[0] + transform[4 + a] * v[1] + transform[8 + a] * v[2]
                    });
                    let len = v.iter().map(|v| v * v).sum::<f32>().sqrt();
                    ensure!(len > 0.00001, "Degenerate native normal");
                    Ok(v.map(|v| v / len))
                };
                let n = vector(vertex.normal)?;
                let t = vector(vertex.tangent[..3].try_into().unwrap())?;
                mesh.attributes.push([
                    vertex.uv[0],
                    vertex.uv[1],
                    n[0],
                    n[1],
                    n[2],
                    1.,
                    t[0],
                    t[1],
                    t[2],
                    vertex.tangent[3],
                ]);
                mesh.weights.push(weights(
                    vertex.joints,
                    vertex.weights,
                    first,
                    model.bones.len(),
                )?);
            }
            mesh.groups.push((
                key,
                primitive
                    .indices
                    .chunks_exact(3)
                    .map(|f| [base + f[0], base + f[1], base + f[2]])
                    .collect(),
            ));
        }
    }
    Ok(Assembly {
        mesh,
        materials,
        rig_report: reports,
        palette,
        groups,
    })
}

fn runtime_models(native: &mut Reader, entity: u32, kind: u16) -> Result<Vec<Value>> {
    let payload = native.tag(entity, Some(0x80809c0f))?;
    let mut models = Vec::new();
    ensure!(
        payload.u16(0x96)? == kind,
        "Requested runtime object type differs from {kind}"
    );
    for row in payload.array(16, 12, None)? {
        let tag = payload.u32(row)?;
        let owner = native.tag(tag, Some(0x80809c36))?;
        let resource = owner.pointer(24)?;
        if owner.u32(resource - 4)? == 0x808072bd {
            models.push(json!({"entity":format!("{entity:08X}"),"owner":format!("{tag:08X}"),"model":format!("{:08X}",owner.u32(resource+0x1dc)?)}));
        }
    }
    ensure!(
        !models.is_empty(),
        "Native runtime has no direct render model"
    );
    Ok(models)
}

fn carrier(
    native: &mut Reader,
    models: &[Value],
) -> Result<(Value, std::sync::Arc<Payload>, usize)> {
    let mut selected = None;
    for model in models {
        let header = native.tag(hash(model, "model")?, Some(0x808073a5))?;
        for row in header.array(16, 136, Some(0x80807378))? {
            let positions = native.tag(header.u32(row)?, None)?;
            let attributes = native.tag(header.u32(row + 4)?, None)?;
            // The carrier contributes native storage classes. Its original vertex
            // declaration is replaced, including the compact vehicle attribute stride.
            if positions.u16(4)? == 0 || attributes.u16(4)? == 0 {
                continue;
            }
            let score = positions.u32(0)?;
            if selected.as_ref().is_none_or(
                |(old, _, _, _): &(u32, Value, std::sync::Arc<Payload>, usize)| score > *old,
            ) {
                selected = Some((score, model.clone(), header.clone(), row));
            }
        }
    }
    let (_, model, header, row) = selected.context("No supported native body carrier")?;
    Ok((model, header, row))
}

/// A hierarchy's root need not be a rendered joint. Packed native vehicle
/// vertices identify the visible rig branch independently of source bone names.
fn drawn_root(
    native: &mut Reader,
    model: &Payload,
    mesh: usize,
    skeleton: &Value,
) -> Result<Option<u8>> {
    if !matches!(model.u16(mesh + 88)?, 137 | 139) {
        return Ok(None);
    }
    let header_tag = model.u32(mesh)?;
    let header = native.tag(header_tag, None)?;
    if header.u16(4)? != 8 {
        return Ok(None);
    }
    let data_tag = native.reference(header_tag)?;
    let data = native.tag(data_tag, None)?;
    ensure!(
        data.0.len() == usize::try_from(header.u32(0)?)? && data.0.len().is_multiple_of(8),
        "Invalid packed native positions"
    );
    let bones = skeleton["bones"].as_array().context("Carrier skeleton")?;
    let mut used = BTreeSet::new();
    for at in (0..data.0.len()).step_by(8) {
        let index = usize::from(data.u16(at + 6)?);
        ensure!(
            index < bones.len(),
            "Native packed vertex bone exceeds its skeleton"
        );
        used.insert(index);
    }
    let path = |index: usize| -> Result<Vec<usize>> {
        let mut path = vec![index];
        let mut parent = bones[index]["parent"]
            .as_i64()
            .context("Native bone parent")?;
        while parent >= 0 {
            let index = usize::try_from(parent)?;
            ensure!(
                index < bones.len() && !path.contains(&index),
                "Native bone hierarchy is cyclic or out of range"
            );
            path.push(index);
            parent = bones[index]["parent"]
                .as_i64()
                .context("Native bone parent")?;
        }
        Ok(path)
    };
    let paths = used
        .iter()
        .map(|&bone| path(bone))
        .collect::<Result<Vec<_>>>()?;
    let first = paths
        .first()
        .context("Native carrier has no weighted vertices")?;
    Ok(first
        .iter()
        .copied()
        .find(|bone| used.contains(bone) && paths.iter().all(|p| p.contains(bone)))
        .map(u8::try_from)
        .transpose()?)
}

fn carrier_models(
    native: &mut Reader,
    template: &Value,
    entry: &ImportRequest,
    projectile: bool,
) -> Result<Vec<Value>> {
    Ok(if projectile {
        runtime_models(
            native,
            entry.projectile_entity.context("Projectile carrier")?,
            18,
        )?
    } else if let Some(entity) = entry.runtime_entity {
        runtime_models(native, entity, 15)?
    } else {
        template["models"]
            .as_array()
            .context("Native models")?
            .clone()
    })
}

fn validate_material_overrides(
    materials: &BTreeMap<u32, crate::halo_reach::material::Material>,
    overrides: &BTreeMap<String, BTreeMap<String, [f32; 4]>>,
) -> Result<()> {
    for (path, values) in overrides {
        let material = materials
            .values()
            .find(|m| m.tag.as_ref().is_some_and(|t| &t.path == path))
            .with_context(|| format!("Material override source {path} is absent"))?;
        for (name, value) in values {
            ensure!(
                material.constant(name).is_some() && value.iter().all(|v| v.is_finite()),
                "Invalid material parameter override {path}: {name}"
            );
        }
    }
    Ok(())
}

fn render_owner(
    native: &mut Reader,
    g: &mut Graph,
    owner_tag: u32,
) -> Result<std::sync::Arc<Payload>> {
    let original = native.tag(owner_tag, Some(0x80809c36))?;
    let resource = original.pointer(24)?;
    ensure!(
        original.u32(resource - 4)? == 0x808072bd,
        "Native carrier is not a render owner"
    );
    let mut owner = original.0.clone();
    let mut patches = vec![json!({"offset":resource+0x1dc,"symbol":"model"})];
    for at in (0..owner.len() - 3).step_by(4) {
        if original.u32(at)? == owner_tag {
            patches.push(json!({"offset":at,"symbol":"owner"}));
        }
    }
    for patch in &patches {
        put(
            &mut owner,
            patch["offset"].as_u64().unwrap() as usize,
            &u32::MAX.to_le_bytes(),
        )?;
    }
    g.add("owner", owner_tag, &owner, None, patches)?;
    Ok(original)
}

fn source_motion(
    native: &mut Reader,
    scene: &Scene,
    entry: &ImportRequest,
    root: &Path,
    entity: u32,
    projectile: bool,
    bones: &mut Value,
) -> Result<Option<super::animation::native::Bundle>> {
    let source_rig = entry.bone_aliases.is_empty()
        && scene.models.len() == 1
        && scene.models[0].animation.is_some();
    if projectile
        || entry.runtime_entity.is_none()
        || (entry.animation_bindings.is_empty() && !source_rig)
    {
        return Ok(None);
    }
    ensure!(
        entry.runtime_entity == Some(entity)
            && entry.recipe["kind"] == "sparrow"
            && entry.bone_aliases.is_empty(),
        "Source animation bindings require an explicit vehicle runtime without bone aliases"
    );
    let motion =
        super::animation::native::prepare(native, scene, entity, &entry.animation_bindings, root)?;
    *bones = motion.skeleton.clone();
    Ok(Some(motion))
}

fn motion_report(
    graph: &mut Value,
    motion: Option<super::animation::native::Bundle>,
    equipment: bool,
) {
    let Some(motion) = motion else {
        return;
    };
    if equipment {
        graph["equipment_animation"] = motion.section;
    }
    graph["source_motion"] = motion.source_motion;
    graph["limits"] = json!([
        "Explicit source clips animate appended native vehicle bones at the carrier's retained timing. Source flight, passenger seats, animation state selection, events and root movement are not translated. In-game playback is unverified."
    ]);
}

fn selected_skeleton(original: Value, extended: Option<&Value>, projectile: bool) -> Result<Value> {
    let Some(extended) = extended else {
        return Ok(original);
    };
    ensure!(
        !projectile
            && extended["owner"] == original["owner"]
            && extended["entity"] == original["entity"],
        "Extended weapon skeleton differs from the selected geometry carrier"
    );
    Ok(extended.clone())
}

fn prepare_optic(
    entry: &ImportRequest,
    scene: &Scene,
    projectile: bool,
    mesh: &mut Mesh,
    materials: &BTreeMap<u32, crate::halo_reach::material::Material>,
) -> Result<Option<optic::Prepared>> {
    if projectile {
        return Ok(None);
    }
    validate_material_overrides(materials, &entry.material_overrides)?;
    optic::select(entry, scene, mesh, materials)
}

fn material_stages(
    materials: &BTreeMap<u32, crate::halo_reach::material::Material>,
    lenses: &BTreeMap<u32, material::Lens>,
) -> BTreeMap<u32, u8> {
    materials
        .iter()
        .map(|(&key, material)| {
            (
                key,
                if material.alpha_mode() == "BLEND" || lenses.contains_key(&key) {
                    7
                } else {
                    0
                },
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
pub(super) fn build(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    native: &mut Reader,
    scene: &Scene,
    entry: &ImportRequest,
    root: &Path,
    projectile: bool,
    weapon_skeleton: Option<&Value>,
) -> Result<Value> {
    let inventory_scene;
    let scene = if !projectile
        && entry.runtime_entity.is_none()
        && entry.recipe["kind"] == "sparrow"
        && scene.damage.is_some()
    {
        inventory_scene = inspection::intact(scene)?;
        &inventory_scene
    } else {
        scene
    };
    let donor = hash(&entry.recipe["donor"], "item_hash")?;
    let item = hash(&entry.recipe["identity"], "item_hash")?;
    let template = crate::tiger::shadowkeep::extract(native, donor)?;
    let models = carrier_models(native, &template, entry, projectile)?;
    let (model, header, row) = carrier(native, &models)?;
    let owner_tag = hash(&model, "owner")?;
    let entity_tag = hash(&model, "entity")?;
    let model_tag = hash(&model, "model")?;
    let mut bones = skeleton(
        native,
        entity_tag,
        if projectile {
            0
        } else {
            hash(&template, "item_tag")?
        },
        usize::try_from(header.u32(0x40)?)?,
    )?;
    bones = selected_skeleton(bones, weapon_skeleton, projectile)?;
    let motion = source_motion(
        native, scene, entry, root, entity_tag, projectile, &mut bones,
    )?;
    if !projectile
        && entry.recipe["kind"] == "sparrow"
        && motion.is_none()
        && let Some(root) = drawn_root(native, &header, row, &bones)?
    {
        bones["fallback_bone"] = json!(root);
    }
    let Assembly {
        mut mesh,
        materials,
        rig_report,
        palette,
        groups,
    } = assemble(scene, &bones, &entry.bone_aliases)?;
    let inspection_fit = inspection::fit(&mut mesh, &header, entry, projectile)?;
    let optic = prepare_optic(entry, scene, projectile, &mut mesh, &materials)?;
    let lenses = optic.as_ref().map(|o| o.lenses.clone()).unwrap_or_default();
    let stages = material_stages(&materials, &lenses);
    let encoded = if scene.damage.is_some() {
        geometry::encode_grouped(&mesh, &header, row, &stages, &groups)?
    } else {
        geometry::encode_with_stages(&mesh, &header, row, &stages)?
    };
    fs::create_dir_all(root)?;
    let mut g = Graph {
        root: root.to_path_buf(),
        nodes: Vec::new(),
    };
    buffers(native, &mut g, &header, row, &encoded)?;
    g.add("model", model_tag, &encoded.header, None, encoded.patches)?;
    let original = render_owner(native, &mut g, owner_tag)?;
    let original_entity = native.tag(entity_tag, Some(0x80809c0f))?;
    let mut entity = original_entity.0.clone();
    let mut patches = Vec::new();
    for at in crate::tiger::entity::owner_slots(&original_entity, &original, owner_tag)? {
        put(&mut entity, at, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":at,"symbol":"owner"}));
    }
    crate::tiger::entity::reject_stale_owner(&Payload(entity.clone()), owner_tag)?;
    let marker_report = if entry.runtime_entity.is_none() && !projectile {
        marker::build(
            native,
            &mut g,
            &scene.models[0].model,
            &mut entity,
            &mut patches,
        )?
    } else {
        Vec::new()
    };
    let driver_seat = if entry.runtime_entity.is_some() && !projectile {
        let source_rider = entry.character.as_ref().is_some_and(|c| c.rider.is_some());
        seat::build(
            native,
            &mut g,
            scene,
            &bones,
            entry.driver_pose.as_ref().filter(|_| !source_rider),
            &mut entity,
            &mut patches,
        )?
    } else {
        None
    };
    if let Some(motion) = &motion {
        motion.link(&mut g, &mut entity, &mut patches)?;
    }
    g.add("entity", entity_tag, &entity, None, patches)?;
    let mut live_display = false;
    for mapping in materials
        .values()
        .flat_map(|m| &m.properties)
        .flat_map(|p| &p.textures)
    {
        live_display |= mapping.ammunition_place(cache)?.is_some();
    }
    let objects = if live_display {
        crate::presentation::channel::inherit(native, &mut g, &[0x32CC5402])?
    } else {
        BTreeMap::new()
    };
    ensure!(
        entry.paint.is_empty() || entry.recipe["kind"] == "sparrow",
        "Source paint currently requires Sparrow dye channels"
    );
    let mut paint = super::paint::prepare(
        native,
        &mut g,
        &materials,
        &entry.paint,
        hash(&template, "item_tag")?,
        item,
    )?;
    if entry.runtime_entity.is_some() {
        // Generic vehicle draws have no gear dye binding callback. A nonempty
        // buffer can belong to an earlier draw, so an empty-buffer fallback is
        // insufficient. Keep item dyes for inventory, but use source colors on
        // the summoned model regardless of its occupancy state.
        paint.surfaces.clear();
    }
    let material_report = material::build(
        cache,
        pages,
        native,
        &mut g,
        &materials,
        material::Binding {
            palette: &palette,
            rigid: bones["rigid_uniforms"] == true,
            objects: &objects,
            overrides: &entry.material_overrides,
            lenses: &lenses,
            paint: &paint.surfaces,
            lightmap: None,
            vertex_data: None,
            statics: false,
            prefix: "",
        },
    )?;
    let instance_layout = g.seal_instances()?;
    let vehicle = if !projectile {
        entry
            .runtime_entity
            .map(|_| vehicle::build(native, &mut g, scene, &bones, entry, cache, pages))
            .transpose()?
            .flatten()
    } else {
        None
    };
    if projectile {
        let graph = json!({"nodes":g.nodes,"source_owner":owner_tag,"source_entity":entity_tag,
            "instance_layout":instance_layout,
            "vertices":mesh.positions.len(),"triangles":mesh.groups.iter().map(|g|g.1.len()).sum::<usize>(),
            "native_skinning":{"source_bones":rig_report,"native_skeleton":bones,"motion_palette":palette},"materials":material_report});
        bind_materials(&graph, root)?;
        return Ok(graph);
    }
    let parents = template["parents"]
        .as_array()
        .context("Native art parents")?;
    let parent = parents
        .iter()
        .find(|p| {
            hex::decode(p["parent_bytes"].as_str().unwrap_or(""))
                .ok()
                .is_some_and(|b| b.get(16..20) == Some(entity_tag.to_le_bytes().as_slice()))
        })
        .or_else(|| parents.first())
        .context("Native art parent")?;
    let parent_tag = hash(parent, "parent")?;
    let mut bytes = native.tag(parent_tag, None)?.0.clone();
    put(&mut bytes, 16, &u32::MAX.to_le_bytes())?;
    g.add(
        "parent",
        parent_tag,
        &bytes,
        None,
        vec![json!({"offset":16,"symbol":"entity"})],
    )?;
    // The authoring linker synthesizes this shared companion from the parent contract.
    let companion = native.tag(0x81a662de, None)?;
    g.add("parent-companion", 0x81a662de, &companion.0, None, vec![])?;
    let node = g.nodes.last_mut().unwrap();
    node["shared_owner"] = json!("parent");
    node["source_parent"] = json!(parent_tag);
    let assignment = hash(parent, "assignment")?;
    let art_key = name_hash(&format!("reach-{item:08X}-art"));
    let optic_report = optic
        .as_ref()
        .map(|prepared| {
            optic::build(
                native,
                &mut g,
                &template,
                prepared,
                item,
                entry.optic_item,
                optic::Source {
                    cache,
                    pages,
                    weapon: &scene.object.tag,
                },
            )
        })
        .transpose()?;
    let mut graph = json!({"source":"halo_reach","reach_revision":REVISION,"item_hash":item,"native_item":donor,"art_key":art_key,"native_assignment":assignment,"source_model":scene.models[0].model.tag.datum,
        "source_owner":owner_tag,"source_entity":entity_tag,"native_model":model_tag,"parent":"parent","companion":"parent-companion","nodes":g.nodes,
        "vertices":mesh.positions.len(),"triangles":mesh.groups.iter().map(|g|g.1.len()).sum::<usize>(),"native_skinning":{"source_bones":rig_report,"native_skeleton":bones,"motion_palette":palette,"method":"Preserved source joints and influences with separate native motion lookup"},
        "materials":material_report,"material_overrides":entry.material_overrides,"markers":marker_report,"object_inputs":objects,"instance_layout":instance_layout,"installable":true,"gameplay_verified":false,
        "limits":["Geometry and skinning use the native runtime. Animation, handling, audio, sights and projectile sections record their translated source behavior and remaining limits."]});
    graph["inspection_fit"] = json!(inspection_fit);
    if let Some((kept, report)) = optic_report {
        graph["kept_parts"] = json!([kept]);
        graph["optic"] = report;
    }
    motion_report(&mut graph, motion, vehicle.is_none());
    if entry.recipe["kind"] == "sparrow" {
        graph["kind"] = json!("sparrow");
        let mut rows = template["art_rows"]
            .as_array()
            .context("Native art rows")?
            .clone();
        for row in &mut rows {
            row["template_index"] = row["art_index"].clone();
        }
        let registrations = art_registrations(parents, item)?;
        graph["gear_art"] =
            json!({"selector_format":"shadowkeep","rows":rows,"parts":registrations});
        graph["dyes"] = json!(paint.dyes);
        graph["dye_rows"] = json!([[], paint.rows, []]);
        graph["paint"] = json!(entry.paint);
        graph["paint_binding"] = json!(if entry.runtime_entity.is_some() {
            "Source colors on the summoned model. Equipped dyes apply to inventory gear art."
        } else {
            "Equipped dyes apply through the inventory gear draw callback."
        });
    }
    runtime_presentation(&mut graph, entry.runtime_entity, owner_tag, driver_seat)?;
    if let Some(vehicle) = vehicle {
        graph["runtime_presentation"]["root"] = json!(vehicle::ROOT);
        graph["vehicle"] = vehicle;
        graph["limits"] = graph["vehicle"]["limits"].clone();
    }
    bind_materials(&graph, root)?;
    write_json(&root.join("asset-graph.json"), &graph)?;
    crate::graph::record_converter_revision(root)?;
    Ok(graph)
}

fn runtime_presentation(
    graph: &mut Value,
    runtime: Option<u32>,
    owner: u32,
    driver_seat: Option<Value>,
) -> Result<()> {
    let Some(runtime) = runtime else {
        return Ok(());
    };
    let nodes = graph["nodes"]
        .as_array()
        .context("Native graph nodes")?
        .iter()
        .filter(|node| {
            !matches!(
                node["symbol"].as_str(),
                Some("parent" | "parent-companion" | "entity")
            ) && !node["symbol"]
                .as_str()
                .is_some_and(|name| name.starts_with("paint-"))
        })
        .cloned()
        .collect::<Vec<_>>();
    graph["runtime_presentation"] = json!({"installable":true,"source_entity":runtime,
        "source_owner":owner,"owner":"owner","nodes":nodes});
    if let Some(seat) = driver_seat {
        graph["driver_seat"] = seat.clone();
        graph["runtime_presentation"]["components"] = json!([seat]);
    }
    Ok(())
}

fn art_registrations(parents: &[Value], item: u32) -> Result<Vec<Value>> {
    let mut registrations = Vec::new();
    let mut assigned = BTreeSet::new();
    for parent in parents {
        let source = hash(parent, "assignment")?;
        if assigned.insert(source) {
            registrations.push(json!({"source_assignment":source,"key":name_hash(&format!("reach-{item:08X}-art-{source:08X}")),"parent":"parent"}));
        }
    }
    Ok(registrations)
}

fn buffers(
    native: &mut Reader,
    g: &mut Graph,
    header: &Payload,
    row: usize,
    encoded: &geometry::Encoded,
) -> Result<()> {
    for (i, name) in ["positions", "attributes", "indices"]
        .into_iter()
        .enumerate()
    {
        let tag = header.u32(row + [0, 4, 16][i])?;
        let mut bytes = native.tag(tag, None)?.0.clone();
        put(
            &mut bytes,
            if i == 2 { 8 } else { 0 },
            &u32::try_from(encoded.streams[i].len())?.to_le_bytes(),
        )?;
        if i == 2 {
            bytes[1] = 1;
        } else {
            put(
                &mut bytes,
                4,
                &(if i == 0 { 16u16 } else { 24u16 }).to_le_bytes(),
            )?;
        }
        let symbol = format!("{name}-header");
        let data = format!("{name}-data");
        g.add(&symbol, tag, &bytes, Some(&data), vec![])?;
        g.add(
            &data,
            native.reference(tag)?,
            &encoded.streams[i],
            Some(&symbol),
            vec![],
        )?;
    }
    Ok(())
}

fn bind_materials(graph: &Value, root: &Path) -> Result<()> {
    for (symbol, bytes) in crate::tiger::material::bind(graph, root)? {
        let node = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["symbol"] == symbol)
            .context("Bound shader node")?;
        fs::write(
            root.join(node["file"].as_str().context("Shader payload")?),
            bytes,
        )?;
    }
    Ok(())
}

fn projectile_count(barrel: &Value, behavior: &Value) -> Result<i64> {
    let mut count = barrel["projectiles_per_shot"]
        .as_i64()
        .context("Source projectile count")?;
    if let Some(datum) = barrel["projectile"]["datum"].as_u64() {
        let projectile = &behavior["nodes"][format!("{datum:08X}")]["data"];
        let flags = projectile["flags"]
            .as_u64()
            .context("Source projectile flags")?;
        let cones = projectile["conical_spread"]
            .as_array()
            .context("Source conical spread")?;
        if flags & (1 << 18) == 0 && !cones.is_empty() {
            ensure!(
                cones.len() == 1,
                "Multiple source conical spreads need explicit translation"
            );
            let yaw = cones[0]["yaw_count"]
                .as_i64()
                .context("Conical yaw count")?;
            let pitch = cones[0]["pitch_count"]
                .as_i64()
                .context("Conical pitch count")?;
            ensure!(yaw > 0 && pitch > 0, "Invalid source conical spread counts");
            count = count
                .checked_mul(yaw)
                .and_then(|c| c.checked_mul(pitch))
                .context("Source conical spread count overflow")?;
        }
    }
    ensure!(
        (1..=32767).contains(&count),
        "Source simultaneous projectile count is outside native limits"
    );
    Ok(count)
}

fn recipe(entry: &ImportRequest, scene: &Scene, behavior: &Value) -> Result<(Value, Value)> {
    let mut recipe = entry.recipe.clone();
    if recipe["kind"] == "sparrow" {
        if recipe["overrides"].is_null() {
            recipe["overrides"] = json!({});
        }
        recipe["overrides"]["rarity"] = json!("exotic");
    }
    let Some(barrels) = scene.object.gameplay["barrels"].as_array() else {
        return Ok((recipe, json!({"status":"Native vehicle behavior retained"})));
    };
    if barrels.len() != 1 {
        return Ok((
            recipe,
            json!({"status":"Source barrel selection needs explicit controller translation"}),
        ));
    }
    let count = projectile_count(&barrels[0], behavior)?;
    if recipe["overrides"]["barrel"]["pellets"].is_number() {
        return Ok((
            recipe,
            json!({"source_projectiles_per_shot":count,"status":"Explicit recipe pellet count retained"}),
        ));
    }
    if recipe["overrides"].is_null() {
        recipe["overrides"] = json!({});
    }
    if recipe["overrides"]["barrel"].is_null() {
        recipe["overrides"]["barrel"] = json!({});
    }
    recipe["overrides"]["barrel"]["pellets"] = json!(count);
    Ok((
        recipe,
        json!({"source_projectiles_per_shot":count,"native":"Permanent Barrel pellet count","limits":["Native spread shape and angle, burst timing, ammunition, damage and handling are retained unless the recipe explicitly edits them."]}),
    ))
}

fn select_projectile(recipe: &mut Value, projectile: &Value) -> Result<()> {
    let root = projectile["root"]
        .as_str()
        .context("Imported projectile has no root")?;
    let selection = json!({"imported":root});
    let current = &recipe["overrides"]["fired_graph"];
    ensure!(
        recipe["overrides"]["projectile"].is_null() && (current.is_null() || *current == selection),
        "A source projectile carrier cannot also select another firing graph or projectile edit"
    );
    if recipe["overrides"].is_null() {
        recipe["overrides"] = json!({});
    }
    recipe["overrides"]["fired_graph"] = selection;
    Ok(())
}

fn validate_requests(entries: &mut [ImportRequest]) -> Result<()> {
    let mut slugs = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for entry in entries {
        // Normal saved weapon recipes omit this default field.
        entry
            .recipe
            .as_object_mut()
            .context("Reach recipe must be an object")?
            .entry("kind")
            .or_insert_with(|| json!("weapon"));
        ensure!(
            !entry.slug.is_empty()
                && entry
                    .slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && slugs.insert(&entry.slug),
            "Invalid or repeated import slug"
        );
        ensure!(
            identities.insert(hash(&entry.recipe["identity"], "item_hash")?),
            "Duplicate authored item identity"
        );
        ensure!(
            matches!(entry.recipe["kind"].as_str(), Some("weapon" | "sparrow")),
            "Reach recipes must be weapons or vehicles"
        );
        ensure!(
            entry.driver_pose.is_none()
                || (entry.runtime_entity.is_some() && entry.recipe["kind"] == "sparrow"),
            "A driver pose requires a vehicle runtime"
        );
        scale::validate(entry.scale)?;
        scale::validate(entry.inspection_scale)?;
    }
    Ok(())
}

/// Convert an explicit batch to pinned native graphs and editable Parhelion recipes.
/// Each entry supplies a normal recipe so donor behavior and private identity stay visible.
#[allow(clippy::cognitive_complexity)]
pub fn prepare(plan: &Path, maps: &Path, native_packages: &Path, output: &Path) -> Result<Value> {
    let maps = maps.canonicalize()?;
    let native_packages = native_packages.canonicalize()?;
    let output = crate::io::outside(
        &crate::io::outside(output, maps.parent().context("Reach install root")?)?,
        native_packages.parent().context("Native install root")?,
    )?;
    ensure!(!output.exists(), "Reach import output already exists");
    let mut requests: Vec<Value> = serde_json::from_slice(&fs::read(plan)?)?;
    for request in &mut requests {
        scale::request_defaults(request)?;
    }
    let mut entries: Vec<ImportRequest> = requests
        .into_iter()
        .map(serde_json::from_value)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(!entries.is_empty(), "Reach import plan is empty");
    validate_requests(&mut entries)?;
    fs::create_dir_all(output.parent().context("Output parent")?)?;
    let temporary = tempfile::Builder::new()
        .prefix("reach-native-")
        .tempdir_in(output.parent().unwrap())?;
    let mut native = Reader::new(&native_packages, &temporary.path().join("native"), false)?;
    let mut pages = resource::Pages::default();
    let mut items = Vec::new();
    for mut entry in entries {
        eprintln!("Translating {}", entry.slug);
        let root = temporary.path().join(&entry.slug);
        let source = root.join("source");
        fs::create_dir_all(&source)?;
        let path = source_path(&maps, &entry.source.cache)?;
        pages.record(&path)?;
        let cache = cache::Cache::open(&path)?;
        let mut scene = scene(&cache, &mut pages, &entry.source)?;
        if entry.character.is_none()
            && (scene.object.tag.group != "vehi" || entry.driver_pose.is_none())
        {
            entry.character = super::animation::character::defaults(
                &cache,
                &scene,
                &mut native,
                hash(&entry.recipe["donor"], "item_hash")?,
            )?;
        }
        gltf::write(&cache, &mut pages, &scene, &source)?;
        write_json(&source.join("source.json"), &serde_json::to_value(&scene)?)?;
        let behavior = behavior::read(&cache, &scene)?;
        if entry.source.audio.is_none()
            && matches!(scene.object.tag.group.as_str(), "weap" | "vehi")
        {
            entry.source.audio = super::audio::defaults(&maps, &behavior)?;
        }
        if let Some(audio) = &mut entry.source.audio
            && audio.bank.as_os_str().is_empty()
        {
            let discovered = super::audio::defaults(&maps, &behavior)?
                .context("Previously imported source audio is no longer available")?;
            audio.bank = discovered.bank;
            audio.additional_banks = discovered.additional_banks;
            audio.decoder = discovered.decoder;
        }
        let (mut recipe, mut gameplay) = recipe(&entry, &scene, &behavior)?;
        let hud = super::hud::prepare(&cache, &mut pages, &scene, &mut recipe, &source)?;
        write_json(&source.join("behavior.json"), &behavior)?;
        let projectiles = super::projectile::export(&cache, &mut pages, &behavior, &source)?;
        let mut projectile_import = super::projectile::select(&mut entry, &behavior, &projectiles)?;
        super::animation::export(&cache, &mut pages, &behavior, &scene, &source)?;
        if entry.first_person.is_none() && entry.character.is_none() {
            entry.first_person = super::animation::first_person::defaults(
                &cache,
                &scene,
                &mut native,
                hash(&entry.recipe["donor"], "item_hash")?,
                &source,
            )?;
            if let Some(options) = &mut entry.first_person {
                options.handling_audio = entry.source.audio.is_some();
            }
        }
        if scene.object.tag.group == "weap"
            && (entry.first_person.is_some()
                || scene.object.gameplay["zoom_levels"]
                    .as_u64()
                    .is_some_and(|levels| levels > 0))
        {
            gameplay["aim"] = super::hud::scope::aim(&cache, &scene.object.tag)?;
        }
        let source_audio = entry
            .source
            .audio
            .as_ref()
            .map(|options| {
                super::audio::export(options, &behavior, &mut pages, &source.join("audio"))
            })
            .transpose()?;
        super::damage::expand(&cache, &mut pages, &mut scene, &entry.source)?;
        if let Some(damage) = &scene.damage {
            write_json(
                &source.join("damage-geometry.json"),
                &serde_json::to_value(damage)?,
            )?;
        }
        let scene = scale::scene(scene, entry.scale)?;
        let animation = entry
            .first_person
            .as_ref()
            .map(|options| {
                super::animation::first_person::prepare(
                    &cache,
                    &scene,
                    &mut native,
                    hash(&entry.recipe["donor"], "item_hash")?,
                    options,
                    &entry.bone_aliases,
                    &root.join("graph"),
                    entry.source.audio.is_some(),
                )
            })
            .transpose()?;
        let mut graph = build(
            &cache,
            &mut pages,
            &mut native,
            &scene,
            &entry,
            &root.join("graph"),
            false,
            animation
                .as_ref()
                .and_then(|a| a.get("equipment_skeleton"))
                .filter(|s| !s.is_null()),
        )
        .with_context(|| format!("Native Reach import {}", entry.slug))?;
        if let Some(audio) = source_audio {
            graph["source_audio"] = json!({
                "decoded_samples":audio["samples"].as_array().map_or(0, Vec::len),
                "unavailable":audio["unavailable"],
                "playback":"Decoded recordings are source evidence. The audio routes describe native playback coverage."
            });
        }
        inspection::build(
            &cache,
            &mut pages,
            &mut native,
            &scene,
            &entry,
            &root.join("graph"),
            &mut graph,
        )?;
        graph["source_gameplay"] = gameplay;
        graph["source_scale"] =
            json!({"length":entry.scale,"mass":1.,"inertia":entry.scale * entry.scale});
        graph["import_settings"] = settings::record(&entry)?;
        graph["source_hud"] = hud;
        if let Some(animation) = animation {
            graph["animation"] = animation;
        }
        if entry.character.is_some() {
            let character = super::animation::character::prepare(
                &cache,
                &mut pages,
                &mut native,
                &entry,
                &mut graph,
                &mut recipe,
                &root.join("graph"),
            )?;
            if !graph["animation"].is_object() {
                graph["animation"] = json!({});
            }
            graph["animation"]["character"] = character;
        }
        if entry.projectile_entity.is_some() {
            graph["projectile"] = super::projectile::native(
                &cache,
                &mut pages,
                &mut native,
                &behavior,
                &projectiles,
                &entry,
                &root.join("graph"),
            )?;
            select_projectile(&mut recipe, &graph["projectile"])?;
            projectile_import["status"] = json!("imported");
        }
        graph["projectile_import"] = projectile_import;
        if let Some(audio) = &entry.source.audio
            && entry.runtime_entity.is_none()
        {
            let template = crate::tiger::shadowkeep::extract(
                &mut native,
                hash(&entry.recipe["donor"], "item_hash")?,
            )?;
            graph["audio"] = match super::audio::native(
                &mut native,
                hash(&template, "item_tag")?,
                &behavior,
                &source.join("audio"),
                &root.join("graph"),
                audio,
            ) {
                Ok(audio) => audio,
                Err(error) if audio.firing.is_some() || audio.require_firing => {
                    return Err(error)
                        .with_context(|| format!("Requested firing audio for {}", entry.slug));
                }
                Err(error) => json!({"status":"source_only","reason":format!("{error:#}")}),
            };
            if audio.firing.is_some() || audio.require_firing {
                ensure!(
                    graph["audio"]["authoring_schema"] == 3,
                    "Requested firing audio was not linked for {}: {}",
                    entry.slug,
                    graph["audio"]
                );
            }
        }
        if entry
            .first_person
            .as_ref()
            .is_some_and(|options| !options.melee.is_empty())
        {
            graph["melee"] = super::melee::prepare(
                &mut native,
                &behavior,
                &mut graph,
                &mut recipe,
                &root.join("graph"),
                entry.source.audio.is_some(),
            )?;
        }
        if entry
            .first_person
            .as_ref()
            .is_some_and(|options| !options.actions.is_empty() && !options.handling_audio)
        {
            graph["handling_audio"] = json!({"status":"source_only","reason":"Handling audio is switched off in the import plan, the donor's cues remain on every clip"});
        }
        if entry
            .first_person
            .as_ref()
            .is_some_and(|options| !options.actions.is_empty() && options.handling_audio)
        {
            graph["handling_audio"] = super::audio::clips::prepare(
                &mut native,
                &behavior,
                &mut graph,
                &root.join("graph"),
            )
            .with_context(|| format!("Handling audio for {}", entry.slug))?;
        }
        report::behavior(&mut graph);
        write_json(&root.join("graph/asset-graph.json"), &graph)?;
        crate::graph::record_converter_revision(&root.join("graph"))?;
        crate::artwork::prepare(
            &root.join("graph"),
            &native_packages,
            entry.icon.unwrap_or_else(|| {
                if recipe["kind"] == "weapon" && scene.object.gameplay["weapon_class"] == "sword" {
                    crate::artwork::Settings::sword()
                } else {
                    crate::artwork::Settings::for_recipe(&recipe)
                }
            }),
        )?;
        crate::artwork::apply(&mut recipe, &root.join("graph"))?;
        report::compact(&root.join("graph/asset-graph.json"))?;
        let item = hash(&entry.recipe["identity"], "item_hash")?;
        let mut reference = crate::GraphReference::new(&root.join("graph"), item)?;
        reference.directory = output.join(&entry.slug).join("graph");
        if recipe.get("overrides").is_none() {
            recipe["overrides"] = json!({});
        }
        recipe["overrides"]["imported_graph"] = serde_json::to_value(reference)?;
        write_json(&root.join("weapon.parhelion.json"), &recipe)?;
        items.push(json!({"slug":entry.slug,"source":entry.source,"recipe":output.join(&entry.slug).join("weapon.parhelion.json"),"vertices":graph["vertices"],"triangles":graph["triangles"],"native_skinning":graph["native_skinning"],"retained_native_behavior":graph["retained_native_behavior"],"limits":graph["limits"]}));
    }
    native.finish()?;
    pages.verify_inputs()?;
    let report = json!({"source":"halo_reach","source_build":"Jun 21 2023 15:35:31","reach_revision":REVISION,"inputs":pages.inputs,"resources":pages.resources,"items":items,"gameplay_verified":false});
    write_json(&temporary.path().join("report.json"), &report)?;
    ensure!(!output.exists(), "Reach output appeared concurrently");
    fs::rename(temporary.path(), &output)?;
    Ok(report)
}
