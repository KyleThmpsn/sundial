//! Uniform lengths across native geometry, attachments and source motion.
use crate::halo_reach::{Scene, rig::Bone};
use anyhow::{Result, ensure};

pub(super) fn one() -> f32 {
    1.
}

pub(super) fn validate(scale: f32) -> Result<()> {
    ensure!(
        scale.is_finite() && (0.1..=10.).contains(&scale),
        "Source scale must be between 0.1 and 10"
    );
    Ok(())
}

pub(in crate::halo_reach) fn bones(bones: &mut [Bone], scale: f32) {
    for bone in bones {
        bone.translation.iter_mut().for_each(|v| *v *= scale);
        bone.inverse_bind[12..15]
            .iter_mut()
            .for_each(|v| *v *= scale);
    }
}

pub(super) fn scene(mut scene: Scene, scale: f32) -> Result<Scene> {
    validate(scale)?;
    scene.scale = scale;
    for placed in &mut scene.models {
        bones(&mut placed.model.bones, scale);
        for marker in &mut placed.model.markers {
            marker.translation.iter_mut().for_each(|v| *v *= scale);
        }
        for primitive in &mut placed.model.primitives {
            for vertex in &mut primitive.vertices {
                vertex.position.iter_mut().for_each(|v| *v *= scale);
            }
        }
        if let Some(parent) = &mut placed.parent {
            parent.transform[12..15]
                .iter_mut()
                .for_each(|v| *v *= scale);
        }
    }
    Ok(scene)
}
