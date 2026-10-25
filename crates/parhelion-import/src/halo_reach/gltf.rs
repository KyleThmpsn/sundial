use super::{
    Scene,
    cache::Cache,
    material::{self, Mapping},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Default)]
struct Buffer {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}
impl Buffer {
    fn accessor(
        &mut self,
        bytes: &[u8],
        component: u32,
        kind: &str,
        count: usize,
        target: Option<u32>,
    ) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let offset = self.bytes.len();
        self.bytes.extend(bytes);
        let mut view = json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()});
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        let v = self.views.len();
        self.views.push(view);
        let index = self.accessors.len();
        self.accessors
            .push(json!({"bufferView":v,"componentType":component,"count":count,"type":kind}));
        index
    }
    fn floats(
        &mut self,
        values: impl IntoIterator<Item = f32>,
        kind: &str,
        count: usize,
        target: Option<u32>,
    ) -> usize {
        self.accessor(
            &values
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
            5126,
            kind,
            count,
            target,
        )
    }
}

#[derive(Default)]
struct Images {
    images: Vec<Value>,
    textures: Vec<Value>,
    lookup: BTreeMap<(u32, usize, bool), usize>,
}

fn texture(
    c: &Cache,
    pages: &mut Pages,
    mapping: &Mapping,
    normal: bool,
    root: &Path,
    output: &mut Images,
) -> Result<Value> {
    let Images {
        images,
        textures,
        lookup,
    } = output;
    let tag = mapping.bitmap.as_ref().context("Missing mapped bitmap")?;
    let selected_frame = mapping.static_frame(c)?;
    let key = (tag.datum, selected_frame, normal);
    let index = if let Some(&index) = lookup.get(&key) {
        index
    } else {
        let image = material::image(c, pages, tag, selected_frame, normal)
            .with_context(|| format!("Decoding {} texture {}", mapping.role, tag.path))?;
        image.save(root)?;
        if mapping.functions.iter().any(|function| function.kind == 7) {
            for frame in 0..c.block(tag.address()? + 124, 56)?.len() {
                if frame != selected_frame {
                    material::image(c, pages, tag, frame, normal)?.save(root)?;
                }
            }
        }
        let image_index = images.len();
        images.push(json!({"uri":format!("{}.png",image.name()),"extras":image}));
        let index = textures.len();
        textures.push(json!({"source":image_index,"sampler":0}));
        lookup.insert(key, index);
        index
    };
    Ok(
        json!({"index":index,"extensions":{"KHR_texture_transform":{"scale":[mapping.transform[0],mapping.transform[1]],"offset":[mapping.transform[2],mapping.transform[3]]}}}),
    )
}

pub(super) fn write(c: &Cache, pages: &mut Pages, scene: &Scene, root: &Path) -> Result<Value> {
    let mut buffer = Buffer::default();
    let mut nodes = vec![
        json!({"name":"Reach Scene","rotation":[-std::f32::consts::FRAC_1_SQRT_2,0.,0.,std::f32::consts::FRAC_1_SQRT_2],"children":[]}),
    ];
    let mut meshes = Vec::new();
    let mut skins = Vec::new();
    let mut materials = Vec::new();
    let mut images = Images::default();
    let mut roots = Vec::new();
    let mut bone_nodes = Vec::new();
    let mut reports = Vec::new();
    for placed in &scene.models {
        crate::cancellation::check()?;
        let model = &placed.model;
        let model_root = nodes.len();
        roots.push(model_root);
        nodes.push(json!({"name":model.tag.path,"children":[],"extras":{"source_datum":model.tag.datum,"selected_permutations":model.selected}}));
        let first_bone = nodes.len();
        bone_nodes.push(first_bone);
        for bone in &model.bones {
            nodes.push(json!({"name":bone.name,"translation":bone.translation,"rotation":bone.rotation,"children":[]}));
        }
        for (i, bone) in model.bones.iter().enumerate() {
            let parent = bone.parent.map_or(model_root, |p| first_bone + p);
            nodes[parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(first_bone + i));
        }
        let ibm = buffer.floats(
            model.bones.iter().flat_map(|b| b.inverse_bind),
            "MAT4",
            model.bones.len(),
            None,
        );
        let skin = skins.len();
        skins.push(json!({"name":model.tag.path,"joints":(first_bone..first_bone+model.bones.len()).collect::<Vec<_>>(),"inverseBindMatrices":ibm}));
        for marker in &model.markers {
            let node = nodes.len();
            nodes.push(json!({"name":format!("marker:{}",marker.name),"translation":marker.translation,"rotation":marker.rotation,"scale":[marker.scale,marker.scale,marker.scale],"extras":{"source_marker":marker}}));
            let parent = marker.bone.map_or(model_root, |b| first_bone + b);
            nodes[parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(node));
        }
        let used = model
            .primitives
            .iter()
            .filter_map(|p| p.material)
            .collect::<std::collections::BTreeSet<_>>();
        let mut material_indices = BTreeMap::new();
        for (index, material) in model
            .materials
            .iter()
            .enumerate()
            .filter(|(i, _)| used.contains(i))
        {
            material_indices.insert(index, materials.len());
            let mut mat = json!({"name":material.tag.as_ref().map(|t|t.path.as_str()).unwrap_or("Unassigned"),"pbrMetallicRoughness":{
                "baseColorFactor":material.constant("albedo_color").unwrap_or([1.;4]).map(|v|v.clamp(0.,1.)),
                "metallicFactor":0.,"roughnessFactor":material.constant("roughness").map_or(0.5,|v|v[0].clamp(0.,1.))},
                "alphaMode":material.alpha_mode(),"doubleSided":true,"extras":{"source_material":material}});
            let mut bound = Vec::new();
            if let Some(mapping) = material.mapping(&["base_map", "base_map_m_0"]) {
                mat["pbrMetallicRoughness"]["baseColorTexture"] =
                    texture(c, pages, mapping, false, root, &mut images)?;
                bound.push(mapping.role.clone());
            }
            if let Some(mapping) = material.mapping(&["bump_map", "bump_map_m_0"]) {
                mat["normalTexture"] = texture(c, pages, mapping, true, root, &mut images)?;
                bound.push(mapping.role.clone());
            }
            if let Some(mapping) = material.mapping(&["self_illum_map", "meter_map"]) {
                mat["emissiveTexture"] = texture(c, pages, mapping, false, root, &mut images)?;
                let tint = material
                    .constant("self_illum_color")
                    .or_else(|| material.constant("self_illum_tint_color"))
                    .unwrap_or([1.; 4]);
                mat["emissiveFactor"] = json!(
                    tint[..3]
                        .iter()
                        .map(|v| v.clamp(0., 1.))
                        .collect::<Vec<_>>()
                );
                bound.push(mapping.role.clone());
            }
            let unbound = material
                .properties
                .iter()
                .flat_map(|p| &p.textures)
                .filter(|t| t.bitmap.is_some() && !bound.contains(&t.role))
                .map(|t| t.role.clone())
                .collect::<Vec<_>>();
            reports.push(json!({"tag":material.tag,"bound_roles":bound,"source_only_roles":unbound,"alpha_mode":material.alpha_mode(),"scope":"Portable surface approximation"}));
            materials.push(mat);
        }
        let mut primitives = Vec::new();
        for primitive in &model.primitives {
            let count = primitive.vertices.len();
            ensure!(
                count > 0 && !primitive.indices.is_empty(),
                "Empty exported primitive"
            );
            let position = buffer.floats(
                primitive.vertices.iter().flat_map(|v| v.position),
                "VEC3",
                count,
                Some(34962),
            );
            let min = std::array::from_fn::<_, 3, _>(|a| {
                primitive
                    .vertices
                    .iter()
                    .map(|v| v.position[a])
                    .fold(f32::INFINITY, f32::min)
            });
            let max = std::array::from_fn::<_, 3, _>(|a| {
                primitive
                    .vertices
                    .iter()
                    .map(|v| v.position[a])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            buffer.accessors[position]["min"] = json!(min);
            buffer.accessors[position]["max"] = json!(max);
            let normal = buffer.floats(
                primitive.vertices.iter().flat_map(|v| v.normal),
                "VEC3",
                count,
                Some(34962),
            );
            let tangent = buffer.floats(
                primitive.vertices.iter().flat_map(|v| v.tangent),
                "VEC4",
                count,
                Some(34962),
            );
            let uv = buffer.floats(
                primitive.vertices.iter().flat_map(|v| v.uv),
                "VEC2",
                count,
                Some(34962),
            );
            let weights = buffer.floats(
                primitive.vertices.iter().flat_map(|v| v.weights),
                "VEC4",
                count,
                Some(34962),
            );
            let joints = buffer.accessor(
                &primitive
                    .vertices
                    .iter()
                    .flat_map(|v| v.joints)
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
                5123,
                "VEC4",
                count,
                Some(34962),
            );
            let indices = buffer.accessor(
                &primitive
                    .indices
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
                5125,
                "SCALAR",
                primitive.indices.len(),
                Some(34963),
            );
            let mut p = json!({"attributes":{"POSITION":position,"NORMAL":normal,"TANGENT":tangent,"TEXCOORD_0":uv,"JOINTS_0":joints,"WEIGHTS_0":weights},"indices":indices,"mode":4,
                "extras":{"source_section":primitive.section,"source_part":primitive.part,"instance":primitive.instance,"source_flags":primitive.flags}});
            if let Some(material) = primitive.material {
                p["material"] = json!(material_indices[&material]);
            }
            primitives.push(p);
        }
        let mesh = meshes.len();
        meshes.push(json!({"name":model.tag.path,"primitives":primitives}));
        let node = nodes.len();
        nodes.push(json!({"name":"Geometry","mesh":mesh,"skin":skin}));
        nodes[model_root]["children"]
            .as_array_mut()
            .unwrap()
            .push(json!(node));
    }
    place(scene, &mut nodes, &roots, &bone_nodes);
    let mut gltf = json!({"asset":{"version":"2.0","generator":"Parhelion Reach Import","extras":{"source_build":c.build,"unit":"metre","source_basis":"X forward, Y left, Z up","root_conversion":"Negative quarter turn about X to glTF Y up"}},
        "extensionsUsed":["KHR_texture_transform"],"scene":0,"scenes":[{"nodes":[0]}],"nodes":nodes,"skins":skins,"meshes":meshes,
        "buffers":[{"uri":"scene.bin","byteLength":buffer.bytes.len()}],"bufferViews":buffer.views,"accessors":buffer.accessors,"materials":materials});
    if !images.images.is_empty() {
        gltf["images"] = json!(images.images);
        gltf["textures"] = json!(images.textures);
        gltf["samplers"] = json!([{"magFilter":9729,"minFilter":9987,"wrapS":10497,"wrapT":10497}]);
    }
    fs::write(root.join("scene.bin"), buffer.bytes)?;
    fs::write(root.join("scene.gltf"), serde_json::to_vec_pretty(&gltf)?)?;
    Ok(json!(reports))
}

fn place(scene: &Scene, nodes: &mut [Value], roots: &[usize], bone_nodes: &[usize]) {
    for (i, placed) in scene.models.iter().enumerate() {
        let parent = if let Some(p) = &placed.parent {
            nodes[roots[i]]["matrix"] = json!(p.transform);
            p.bone.map_or(roots[p.model], |b| bone_nodes[p.model] + b)
        } else {
            0
        };
        nodes[parent]["children"]
            .as_array_mut()
            .unwrap()
            .push(json!(roots[i]));
    }
    for node in nodes {
        if node["children"].as_array().is_some_and(Vec::is_empty) {
            node.as_object_mut().unwrap().remove("children");
        }
    }
}
