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
mod optic;
mod seat;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Optic {
    /// The source material whose geometry bounds define the sight aperture.
    pub aperture: String,
    /// All source lens materials that must transmit the native scene.
    pub lenses: Vec<String>,
    /// Camera distance behind the rear aperture, in native metres.
    pub eye_relief: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sights {
    /// Source model sight points in native metres, with X forward and Z up.
    pub rear: [f32; 3],
    pub front: [f32; 3],
    /// Camera distance behind the rear sight along its axis.
    pub eye_relief: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub slug: String,
    pub source: ExportRequest,
    /// A normal Parhelion recipe supplies the donor and private authored identity.
    pub recipe: Value,
    /// Explicit native vehicle entity. It must agree with the recipe's summon choice.
    #[serde(default)]
    pub runtime_entity: Option<u32>,
    /// An explicit native projectile carrier for a single visible source projectile.
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
    /// Explicit appearance state for source parameters whose events are not translated.
    #[serde(default)]
    pub material_overrides: BTreeMap<String, BTreeMap<String, [f32; 4]>>,
    #[serde(default)]
    pub optic: Option<Optic>,
    /// Explicit iron sights for a source that has no lens aperture.
    #[serde(default)]
    pub sights: Option<Sights>,
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
    materials: BTreeMap<u32, material::Material>,
    rig_report: Vec<Value>,
    palette: Vec<u8>,
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
    for placed in &scene.models {
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
            let mat = primitive
                .material
                .and_then(|i| model.materials.get(i))
                .cloned()
                .unwrap_or(material::Material {
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
    if model.u16(mesh + 88)? != 137 {
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
    materials: &BTreeMap<u32, material::Material>,
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

pub(super) fn build(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    native: &mut Reader,
    scene: &Scene,
    entry: &ImportRequest,
    root: &Path,
    projectile: bool,
) -> Result<Value> {
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
    let motion = if !projectile && !entry.animation_bindings.is_empty() {
        ensure!(
            entry.runtime_entity == Some(entity_tag)
                && entry.recipe["kind"] == "sparrow"
                && entry.bone_aliases.is_empty(),
            "Source animation bindings require an explicit vehicle runtime without bone aliases"
        );
        let motion = super::animation::native::prepare(
            native,
            scene,
            entity_tag,
            &entry.animation_bindings,
            root,
        )?;
        bones = motion.skeleton.clone();
        Some(motion)
    } else {
        None
    };
    if !projectile
        && entry.runtime_entity.is_some()
        && motion.is_none()
        && let Some(root) = drawn_root(native, &header, row, &bones)?
    {
        bones["fallback_bone"] = json!(root);
    }
    let Assembly {
        mesh,
        materials,
        rig_report,
        palette,
    } = assemble(scene, &bones, &entry.bone_aliases)?;
    if !projectile {
        validate_material_overrides(&materials, &entry.material_overrides)?;
    }
    let optic = if !projectile {
        optic::select(entry, &mesh, &materials)?
    } else {
        None
    };
    let lenses = optic.as_ref().map(|o| o.lenses.clone()).unwrap_or_default();
    let stages = materials
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
        .collect();
    let encoded = geometry::encode_with_stages(&mesh, &header, row, &stages)?;
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
        super::native_marker::build(
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
        seat::build(native, &mut g, scene, &bones, &mut entity, &mut patches)?
    } else {
        None
    };
    if let Some(motion) = &motion {
        motion.link(&mut g, &mut entity, &mut patches)?;
    }
    g.add("entity", entity_tag, &entity, None, patches)?;
    let live_display = materials
        .values()
        .flat_map(|m| &m.properties)
        .flat_map(|p| &p.textures)
        .any(|mapping| mapping.ammunition_place().is_some());
    let objects = if live_display {
        crate::presentation::channel::inherit(native, &mut g, &[0xCAF915D8])?
    } else {
        BTreeMap::new()
    };
    let material_report = super::native_material::build(
        cache,
        pages,
        native,
        &mut g,
        &materials,
        super::native_material::Binding {
            palette: &palette,
            rigid: bones["rigid_uniforms"] == true,
            objects: &objects,
            overrides: &entry.material_overrides,
            lenses: &lenses,
        },
    )?;
    let instance_layout = g.seal_instances()?;
    if projectile {
        let graph = json!({"nodes":g.nodes,"source_owner":owner_tag,"source_entity":entity_tag,
            "instance_layout":instance_layout,
            "vertices":mesh.positions.len(),"triangles":mesh.groups.iter().map(|g|g.1.len()).sum::<usize>(),
            "native_skinning":{"source_bones":rig_report,"native_skeleton":bones,"motion_palette":palette},"materials":material_report});
        repair(&graph, root)?;
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
        .map(|prepared| optic::build(native, &mut g, &template, prepared, item))
        .transpose()?;
    let mut graph = json!({"source":"halo_reach","reach_revision":REVISION,"item_hash":item,"native_item":donor,"art_key":art_key,"native_assignment":assignment,"source_model":scene.models[0].model.tag.datum,
        "source_owner":owner_tag,"source_entity":entity_tag,"native_model":model_tag,"parent":"parent","companion":"parent-companion","nodes":g.nodes,
        "vertices":mesh.positions.len(),"triangles":mesh.groups.iter().map(|g|g.1.len()).sum::<usize>(),"native_skinning":{"source_bones":rig_report,"native_skeleton":bones,"motion_palette":palette,"method":"Preserved source joints and influences with separate native motion lookup"},
        "materials":material_report,"material_overrides":entry.material_overrides,"markers":marker_report,"object_inputs":objects,"instance_layout":instance_layout,"installable":true,"gameplay_verified":false,
        "limits":["Source joints and weights use approximate native donor motion. Reach animation clips, vehicle physics, passenger seats and dynamic shader effects remain source-only. Driver attachment, audio and projectile sections report their own supported routes and remaining behavior limits."]});
    if let Some((kept, report)) = optic_report {
        graph["kept_parts"] = json!([kept]);
        graph["optic"] = report;
    }
    if let Some(motion) = motion {
        graph["equipment_animation"] = motion.section;
        graph["source_motion"] = motion.source_motion;
        graph["limits"] = json!([
            "Explicit source clips animate appended native vehicle bones at the carrier's retained timing. Source flight, passenger seats, animation state selection, events and root movement are not translated. In-game playback is unverified."
        ]);
    }
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
        graph["gear_art"] = json!({"rows":rows,"parts":registrations});
        graph["dyes"] = json!([]);
        graph["dye_rows"] = json!([[], [], []]);
    }
    runtime_presentation(&mut graph, entry.runtime_entity, owner_tag, driver_seat)?;
    repair(&graph, root)?;
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
            )
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

fn repair(graph: &Value, root: &Path) -> Result<()> {
    for (symbol, bytes) in crate::tiger::vertex_input::repair(graph, root)? {
        let node = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["symbol"] == symbol)
            .context("Repaired shader node")?;
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

/// Convert an explicit batch to pinned native graphs and editable Parhelion recipes.
/// Each entry supplies a normal recipe so donor behavior and private identity stay visible.
pub fn prepare(plan: &Path, maps: &Path, native_packages: &Path, output: &Path) -> Result<Value> {
    let maps = maps.canonicalize()?;
    let native_packages = native_packages.canonicalize()?;
    let output = crate::io::outside(
        &crate::io::outside(output, maps.parent().context("Reach install root")?)?,
        native_packages.parent().context("Native install root")?,
    )?;
    ensure!(!output.exists(), "Reach import output already exists");
    let entries: Vec<ImportRequest> = serde_json::from_slice(&fs::read(plan)?)?;
    ensure!(!entries.is_empty(), "Reach import plan is empty");
    let mut slugs = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for entry in &entries {
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
    }
    fs::create_dir_all(output.parent().context("Output parent")?)?;
    let temporary = tempfile::Builder::new()
        .prefix("reach-native-")
        .tempdir_in(output.parent().unwrap())?;
    let mut native = Reader::new(&native_packages, &temporary.path().join("native"), false)?;
    let mut pages = resource::Pages::default();
    let mut items = Vec::new();
    for entry in entries {
        eprintln!("Translating {}", entry.slug);
        let root = temporary.path().join(&entry.slug);
        let source = root.join("source");
        fs::create_dir_all(&source)?;
        let path = source_path(&maps, &entry.source.cache)?;
        pages.record(&path)?;
        let cache = cache::Cache::open(&path)?;
        let scene = scene(&cache, &mut pages, &entry.source)?;
        gltf::write(&cache, &mut pages, &scene, &source)?;
        write_json(&source.join("source.json"), &serde_json::to_value(&scene)?)?;
        let behavior = behavior::read(&cache, &scene)?;
        let (mut recipe, gameplay) = recipe(&entry, &scene, &behavior)?;
        write_json(&source.join("behavior.json"), &behavior)?;
        let projectiles = super::projectile::export(&cache, &mut pages, &behavior, &source)?;
        super::animation::export(&cache, &mut pages, &behavior, &scene, &source)?;
        if let Some(options) = &entry.source.audio {
            super::audio::export(options, &behavior, &mut pages, &source.join("audio"))?;
        }
        let mut graph = build(
            &cache,
            &mut pages,
            &mut native,
            &scene,
            &entry,
            &root.join("graph"),
            false,
        )
        .with_context(|| format!("Native Reach import {}", entry.slug))?;
        graph["source_gameplay"] = gameplay;
        if let Some(options) = &entry.first_person {
            graph["animation"] = super::animation::first_person::prepare(
                &cache,
                &scene,
                &mut native,
                hash(&entry.recipe["donor"], "item_hash")?,
                options,
                &root.join("graph"),
            )?;
        }
        if entry.projectile_entity.is_some() {
            ensure!(
                recipe["overrides"].get("fired_graph").is_none()
                    && recipe["overrides"].get("projectile").is_none(),
                "A source projectile carrier cannot also select an explicit firing graph or projectile edit"
            );
            graph["projectile"] = super::projectile::native(
                &cache,
                &mut pages,
                &mut native,
                &behavior,
                &projectiles,
                &entry,
                &root.join("graph"),
            )?;
            if recipe.get("overrides").is_none() {
                recipe["overrides"] = json!({});
            }
            recipe["overrides"]["fired_graph"] = json!({"imported":graph["projectile"]["root"]});
        }
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
                Err(error) => json!({"status":"source_only","reason":format!("{error:#}")}),
            };
        }
        write_json(&root.join("graph/asset-graph.json"), &graph)?;
        crate::graph::record_converter_revision(&root.join("graph"))?;
        if recipe["kind"] == "weapon" {
            crate::artwork::prepare(
                &root.join("graph"),
                &native_packages,
                entry.icon.unwrap_or_else(|| {
                    if scene.object.gameplay["weapon_class"] == "sword" {
                        crate::artwork::Settings::sword()
                    } else {
                        crate::artwork::Settings::for_recipe(&recipe)
                    }
                }),
            )?;
            crate::artwork::apply(&mut recipe, &root.join("graph"))?;
        }
        let item = hash(&entry.recipe["identity"], "item_hash")?;
        let mut reference = crate::GraphReference::new(&root.join("graph"), item)?;
        reference.directory = output.join(&entry.slug).join("graph");
        if recipe.get("overrides").is_none() {
            recipe["overrides"] = json!({});
        }
        recipe["overrides"]["imported_graph"] = serde_json::to_value(reference)?;
        write_json(&root.join("weapon.parhelion.json"), &recipe)?;
        items.push(json!({"slug":entry.slug,"source":entry.source,"recipe":output.join(&entry.slug).join("weapon.parhelion.json"),"vertices":graph["vertices"],"triangles":graph["triangles"],"native_skinning":graph["native_skinning"],"limits":graph["limits"]}));
    }
    native.finish()?;
    pages.verify_inputs()?;
    let report = json!({"source":"halo_reach","source_build":"Jun 21 2023 15:35:31","reach_revision":REVISION,"inputs":pages.inputs,"resources":pages.resources,"items":items,"gameplay_verified":false});
    write_json(&temporary.path().join("report.json"), &report)?;
    ensure!(!output.exists(), "Reach output appeared concurrently");
    fs::rename(temporary.path(), &output)?;
    Ok(report)
}
