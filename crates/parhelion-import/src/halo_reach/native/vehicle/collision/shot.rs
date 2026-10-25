//! Damage sections and animated bone frames for the source's separate hit meshes.
use super::*;
use std::collections::BTreeMap;

pub(super) struct Mesh {
    pub name: String,
    pub section: String,
    pub bone: usize,
    pub condition: (String, Vec<u8>),
    pub triangles: Vec<[[f32; 3]; 3]>,
}

pub(super) fn build(scene: &Scene, bones: &Value) -> Result<Vec<Mesh>> {
    let damage = &scene.object.gameplay["damage"];
    let source = &scene.object.gameplay["rigid_body"]["shot_collision"];
    let sections = damage["sections"]
        .as_array()
        .context("Source damage sections")?;
    let materials = damage["materials"]
        .as_array()
        .context("Source damage materials")?;
    let source_materials = source["materials"]
        .as_array()
        .context("Source collision materials")?;
    let first = usize::try_from(
        bones["source_bone_first"]
            .as_u64()
            .context("Source collision skeleton")?,
    )?;
    let model = &scene.models[0].model;
    let world = rig::world(&model.bones)?;
    let mut out = Vec::new();
    for form in body::forms(scene, "collision")? {
        let permutation = &source["regions"][form.source_region]["permutations"][form.permutation];
        for (bsp_index, bsp) in permutation["bsps"]
            .as_array()
            .context("Source collision BSPs")?
            .iter()
            .enumerate()
        {
            let node = usize::try_from(bsp["node"].as_u64().context("Collision BSP node")?)?;
            let name = source["nodes"][node]
                .as_str()
                .context("Collision node name")?;
            let matches = model
                .bones
                .iter()
                .enumerate()
                .filter(|(_, bone)| bone.name == name)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "Collision node is absent or ambiguous in the render rig"
            );
            let bone = matches[0].0 + first;
            let mut grouped = BTreeMap::<usize, Vec<[[f32; 3]; 3]>>::new();
            for surface in bsp["surfaces"]
                .as_array()
                .context("Source collision surfaces")?
            {
                let material = usize::try_from(
                    surface["material"]
                        .as_u64()
                        .context("Source collision material")?,
                )?;
                let source_name = source_materials
                    .get(material)
                    .and_then(Value::as_str)
                    .context("Source hit material name")?;
                // MCC can retain duplicate material rows whose compiled index
                // aliases the first row with that name. All aliases must agree
                // on the damage section and resolve back to the same material.
                let matches = materials
                    .iter()
                    .filter(|m| m["name"] == source_name)
                    .collect::<Vec<_>>();
                ensure!(
                    !matches.is_empty(),
                    "Hit material has no model damage mapping"
                );
                for mapped in &matches {
                    let compiled = usize::try_from(
                        mapped["collision_material_index"]
                            .as_u64()
                            .context("Compiled hit material index")?,
                    )?;
                    ensure!(
                        source_materials.get(compiled).and_then(Value::as_str) == Some(source_name)
                            && mapped["section"] == matches[0]["section"],
                        "Compiled hit material aliases disagree"
                    );
                }
                let mut section = matches[0]["section"]
                    .as_i64()
                    .context("Hit material damage section")?;
                if section == -1 {
                    section = damage["indirect_section"]
                        .as_i64()
                        .context("Indirect damage section")?;
                }
                let section = usize::try_from(section)?;
                ensure!(
                    section < sections.len(),
                    "Hit material damage section exceeds table"
                );
                let points = surface["points_source"]
                    .as_array()
                    .context("Source hit polygon")?
                    .iter()
                    .map(|p| {
                        let position = rig::point(
                            &world[bone - first],
                            vector::<3>(p)?.map(|v| v * scene.metres()),
                            true,
                        );
                        Ok(super::super::super::seat::frame(
                            bones,
                            bone,
                            (position, [0., 0., 0., 1.]),
                        )?
                        .0)
                    })
                    .collect::<Result<Vec<_>>>()?;
                ensure!(points.len() >= 3, "Source hit polygon is empty");
                for index in 1..points.len() - 1 {
                    grouped.entry(section).or_default().push([
                        points[0],
                        points[index],
                        points[index + 1],
                    ]);
                }
            }
            for (section, triangles) in grouped {
                let section = sections[section]["name"]
                    .as_str()
                    .context("Source hit section name")?
                    .to_owned();
                out.push(Mesh {
                    name: format!(
                        "reach_hit_{}_{}_{}_{}",
                        form.source_region, form.permutation, bsp_index, section
                    ),
                    section,
                    bone,
                    condition: (form.region.clone(), form.states.clone()),
                    triangles,
                });
            }
        }
    }
    ensure!(
        !out.is_empty(),
        "Source vehicle has no reachable hit geometry"
    );
    Ok(out)
}
