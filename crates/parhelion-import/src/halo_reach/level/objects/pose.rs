//! Bake a source scenery pose into its vertices before applying its world placement.
use super::super::super::{animation, cache::Cache, model::Model, resource::Pages, rig};
use super::super::mission::pose::Pose;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(super) fn apply(
    cache: &Cache,
    pages: &mut Pages,
    model: &mut Model,
    pose: &Pose,
) -> Result<()> {
    let graph = cache
        .tags
        .get((pose.graph & 0xffff) as usize)
        .and_then(Option::as_ref)
        .filter(|tag| tag.datum == pose.graph && tag.group == "jmad")
        .context("Scenery pose does not name a source animation graph")?;
    let nodes = animation::pose(cache, pages, graph, &pose.clip)?
        .with_context(|| format!("Scenery pose {} is absent from {}", pose.clip, graph.path))?;
    let mut named = BTreeMap::new();
    for node in nodes {
        ensure!(
            named.insert(node.name.clone(), node).is_none(),
            "Duplicate scenery pose node name"
        );
    }
    let mut bones = model.bones.clone();
    let mut changed = 0;
    for bone in &mut bones {
        if let Some(node) = named.get(&bone.name) {
            if let Some(rotation) = node.rotation {
                bone.rotation = rig::quaternion(rotation)?;
                changed += 1;
            }
            if let Some(translation) = node.translation {
                bone.translation = translation;
                changed += 1;
            }
        }
    }
    ensure!(
        changed > 0,
        "Scenery pose {} has no channels for its model",
        pose.clip
    );
    let matrices = rig::world(&bones)?
        .iter()
        .zip(&model.bones)
        .map(|(world, bone)| rig::multiply(world, &bone.inverse_bind))
        .collect::<Vec<_>>();
    for vertex in model
        .primitives
        .iter_mut()
        .flat_map(|primitive| &mut primitive.vertices)
    {
        let mut position = [0.; 3];
        let mut normal = [0.; 3];
        let mut tangent = [0.; 3];
        let mut total = 0.;
        for (&joint, &weight) in vertex.joints.iter().zip(&vertex.weights) {
            ensure!(
                weight.is_finite() && weight >= 0.,
                "Invalid scenery pose weight"
            );
            if weight == 0. {
                continue;
            }
            let matrix = matrices
                .get(usize::from(joint))
                .context("Scenery pose joint outside its model")?;
            let p = rig::point(matrix, vertex.position, true);
            let n = rig::point(matrix, vertex.normal, false);
            let t = rig::point(matrix, vertex.tangent[..3].try_into()?, false);
            for axis in 0..3 {
                position[axis] += p[axis] * weight;
                normal[axis] += n[axis] * weight;
                tangent[axis] += t[axis] * weight;
            }
            total += weight;
        }
        ensure!(
            total > 0. && (total - 1.).abs() < 0.02,
            "Scenery pose weights do not sum to one"
        );
        vertex.position = position.map(|value| value / total);
        vertex.normal = rig::normalize(normal)?;
        vertex.tangent[..3].copy_from_slice(&rig::normalize(tangent)?);
    }
    Ok(())
}
