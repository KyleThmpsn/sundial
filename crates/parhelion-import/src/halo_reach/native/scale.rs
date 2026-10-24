//! Uniform lengths across native geometry, attachments and source motion.
use crate::{
    graph::GraphReference,
    halo_reach::{ImportRequest, Scene, rig::Bone},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub(in crate::halo_reach) const VEHICLE_WORLD: f32 = 0.8;
pub(in crate::halo_reach) const VEHICLE_INSPECTION: f32 = 2.;

impl ImportRequest {
    /// Keep authored dimensions through portable vehicle reimports. Older graphs
    /// did not distinguish explicit one from the browser's implicit one defaults.
    pub fn retain_vehicle_scale(&mut self, reference: &GraphReference) -> Result<()> {
        ensure!(
            self.recipe["kind"] == "sparrow",
            "Scale retention requires a vehicle"
        );
        reference.validate_reusable("sparrow")?;
        let graph: Value = serde_json::from_slice(&std::fs::read(
            reference.directory.join("asset-graph.json"),
        )?)?;
        ensure!(
            graph["source"] == "halo_reach",
            "Saved vehicle assets are not from Halo: Reach"
        );
        let settings = graph.get("import_settings");
        let read = |key: &str, legacy: &Value, fallback: f32| -> Result<f32> {
            let value = if let Some(settings) = settings {
                settings[key]
                    .as_f64()
                    .with_context(|| format!("Saved vehicle {key} is missing or invalid"))?
                    as f32
            } else if legacy.is_null() {
                fallback
            } else {
                let value = legacy
                    .as_f64()
                    .with_context(|| format!("Saved vehicle {key} is invalid"))?
                    as f32;
                if value == 1. { fallback } else { value }
            };
            validate(value)?;
            Ok(value)
        };
        let world = read("scale", &graph["source_scale"]["length"], VEHICLE_WORLD)?;
        let inspection = read(
            "inspection_scale",
            &graph["inspection"]["fit"]["display_scale"],
            VEHICLE_INSPECTION,
        )?;
        self.scale = world;
        self.inspection_scale = inspection;
        Ok(())
    }
}

/// Resolve omitted batch fields before deserialization so explicit one is retained.
pub(super) fn request_defaults(entry: &mut Value) -> Result<()> {
    let vehicle = entry["source"]["group"] == "vehi" || entry["recipe"]["kind"] == "sparrow";
    let entry = entry
        .as_object_mut()
        .context("Reach import entry must be an object")?;
    entry
        .entry("scale")
        .or_insert_with(|| serde_json::json!(if vehicle { VEHICLE_WORLD } else { 1. }));
    entry
        .entry("inspection_scale")
        .or_insert_with(|| serde_json::json!(if vehicle { VEHICLE_INSPECTION } else { 1. }));
    Ok(())
}

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
