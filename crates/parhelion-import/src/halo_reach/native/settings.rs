//! Keep conversion choices with their pinned output for subsequent library refreshes.
use super::*;

const PRESENTATION: &[&str] = &[
    "bone_aliases",
    "animation_bindings",
    "first_person",
    "character",
    "driver_pose",
    "material_overrides",
    "paint",
    "optic",
    "sights",
    "optic_item",
];

pub(super) fn record(entry: &ImportRequest) -> Result<Value> {
    let mut settings = serde_json::to_value(entry)?;
    settings
        .as_object_mut()
        .context("Import settings")?
        .retain(|key, _| {
            PRESENTATION.contains(&key.as_str())
                || matches!(key.as_str(), "scale" | "inspection_scale")
        });
    settings["source_audio"] = serde_json::to_value(&entry.source.audio)?;
    Ok(settings)
}

impl ImportRequest {
    /// Keep vehicle conversion choices, including independently routed engine and firing audio.
    pub fn retain_vehicle_presentation(&mut self, reference: &crate::GraphReference) -> Result<()> {
        self.retain_vehicle_scale(reference)?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        let mut request = serde_json::to_value(&*self)?;
        if let Some(settings) = graph.get("import_settings") {
            for key in PRESENTATION {
                if let Some(value) = settings.get(key) {
                    request[key] = value.clone();
                }
            }
            if let Some(audio) = settings.get("source_audio") {
                request["source"]["audio"] = audio.clone();
            }
        }
        let engine = &graph["vehicle"]["audio"];
        let firing = &graph["vehicle"]["fire_audio"]["sound"];
        if request["source"]["audio"].is_null()
            && (engine["recordings"].as_u64().is_some_and(|n| n > 0) || firing.is_object())
        {
            request["source"]["audio"] = json!({
                "bank":"", "decoder":"", "record_missing_samples":true,
                "fire_bus":firing["output_bus"]
            });
        }
        *self = serde_json::from_value(request).context("Saved vehicle presentation settings")?;
        Ok(())
    }

    /// Read validated saved conversion choices without changing authored item identity.
    pub fn retain_weapon_presentation(&mut self, reference: &crate::GraphReference) -> Result<()> {
        ensure!(
            self.recipe["kind"].is_null() || self.recipe["kind"] == "weapon",
            "Weapon presentation retention requires a weapon"
        );
        reference.validate(hash(&self.recipe["identity"], "item_hash")?)?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        ensure!(
            graph["source"] == "halo_reach",
            "Saved weapon assets are not from Halo: Reach"
        );
        let mut request = serde_json::to_value(&*self)?;
        if self.projectile_entity.is_none()
            && self.recipe["overrides"]["fired_graph"]
                == json!({"imported":"reach-projectile-entity"})
            && let Some(carrier) = graph["projectile"]["conversion"]["native_carrier"].as_u64()
        {
            request["projectile_entity"] = json!(u32::try_from(carrier)?);
        }
        if let Some(settings) = graph.get("import_settings") {
            let settings = settings
                .as_object()
                .context("Saved import settings must be an object")?;
            for key in PRESENTATION
                .iter()
                .copied()
                .chain(["scale", "inspection_scale"])
            {
                if let Some(value) = settings.get(key) {
                    request[key] = value.clone();
                }
            }
            if let Some(audio) = settings.get("source_audio") {
                request["source"]["audio"] = audio.clone();
            }
        }
        // Older graphs recorded the actual translated routes but not their request.
        // Recover those exact clip choices instead of replacing them with browser defaults.
        if graph["import_settings"].get("first_person").is_none() {
            let motion = &graph["animation"]["first_person"]["arm_motion"];
            if !motion.is_null() {
                let mut actions = serde_json::Map::new();
                for action in motion["actions"].as_array().context("Saved arm actions")? {
                    let state = action["state"].as_str().context("Saved arm action state")?;
                    ensure!(
                        actions
                            .insert(
                                state.to_owned(),
                                action
                                    .get("source_selection")
                                    .unwrap_or(&action["source_clip"])
                                    .clone()
                            )
                            .is_none(),
                        "Saved arm action is repeated"
                    );
                }
                let mut melee = BTreeSet::new();
                for strike in graph["animation"]["first_person"]["melee"]["strikes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    melee.insert(
                        strike["source_clip"]
                            .as_str()
                            .context("Saved melee source clip")?,
                    );
                }
                request["first_person"] = json!({
                    "arms_model":motion["arms_model"], "actions":actions, "melee":melee,
                    "held_offset":motion.get("held_offset").cloned().unwrap_or(json!([0., 0., 0.])),
                    "framing":motion["framing"].get("mode").cloned().unwrap_or(json!("source_centered")),
                    "handling_audio":graph["handling_audio"]["status"].as_str().is_some_and(|status| status != "source_only"),
                });
            }
        }
        let audio = &graph["audio"];
        let firing = audio["firing_events"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|event| {
                matches!(
                    event["mapping"].as_str(),
                    Some("source_primary_fire" | "explicit_source_loop")
                )
            });
        let named = audio["matched_events"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|event| event["mapping"] == "source_primary_fire");
        if audio["authoring_schema"] == 3 && (firing.is_some() || named.is_some()) {
            if request["source"]["audio"].is_null() {
                // The current source installation supplies bank paths at conversion time.
                request["source"]["audio"] = json!({
                    "bank":"", "decoder":"", "record_missing_samples":true
                });
            }
            let retained = &mut request["source"]["audio"];
            retained["require_firing"] = json!(true);
            retained["fire_bus"] = audio["output_bus"].clone();
            if let Some(event) = named {
                retained["firing"] = json!({"mode":"named", "event":u32::from_str_radix(
                    event["hash"].as_str().context("Saved firing event")?, 16)?});
            } else if let Some(event) = firing
                && event["sounds"][0]["routing"] == "source_loop"
            {
                retained["firing"] = json!({"mode":"loop",
                    "source":event["sounds"][0]["source"]["path"],
                    "slot":event["native_sounds"][0]["root_slot"],
                    "node":u32::from_str_radix(event["native_sounds"][0]["node_name"].as_str()
                        .context("Saved firing loop node")?, 16)?});
            }
        }
        let retained: ImportRequest =
            serde_json::from_value(request).context("Saved weapon presentation settings")?;
        scale::validate(retained.scale)?;
        scale::validate(retained.inspection_scale)?;
        *self = retained;
        Ok(())
    }
}
