//! Validate a complete private rig before choosing its geometry mapping.
use super::*;

pub(super) struct Source {
    pub config: Value,
    pub mapping: Value,
    animation: Value,
    staging: tempfile::TempDir,
}

impl Source {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        row: &Value,
        donor: u32,
        glaive: bool,
        modern: &Path,
        native: &Path,
        source_path: &Path,
        native_path: &Path,
        source_rig: &Value,
        native_rig: &Value,
        folder: &Path,
    ) -> Result<Self> {
        let pairs = rig_convert::owned::validate(source_rig, native_rig)?;
        let (from, to, _) = pairs
            .iter()
            .find(|(_, _, fp)| !fp)
            .context("equipment rig")?;
        let config = json!({"source":source_path,"native":native_path,
            "source_geometry":source_path,"source_owner":format!("{from:08X}"),
            "native_owner":format!("{to:08X}"),"source_owned":true});
        let mapping =
            rig_convert::compatible_map(&config, &source_rig["item_tag"], &native_rig["item_tag"])?;
        // Failed private conversion must not leave payloads in the native fallback graph.
        // The temporary graph is published only after the whole animation chain validates.
        let staging = tempfile::Builder::new()
            .prefix("source-rig-")
            .tempdir_in(folder)?;
        let calibration = match &row["rig_calibration"] {
            Value::String(path) => serde_json::from_slice::<Value>(&fs::read(path)?)?,
            Value::Null if glaive => {
                ensure!(
                    donor == super::super::glaive::BASE,
                    "the glaive calibration requires its native base"
                );
                super::super::glaive::calibration()?
            }
            Value::Null => {
                rig_convert::owned::calibration(modern, donor, &staging.path().join("calibration"))?
            }
            _ => anyhow::bail!("rig calibration must name an export"),
        };
        let animation = rig_convert::animation::first_person::prepare_with_rig(
            modern,
            native,
            source_rig,
            native_rig,
            &staging.path().join("work"),
            &staging.path().join("graph"),
            Some(&calibration),
        )?;
        ensure!(
            animation["first_person_status"] == "linked"
                && animation["first_person"]["rigs"]
                    .as_array()
                    .is_some_and(|rigs| {
                        rigs.iter()
                            .filter(|rig| rig.get("kind").is_none() || rig["kind"] == "skeleton")
                            .count()
                            == 2
                    }),
            "source rig animation chain is incomplete: {}",
            animation["reason"]
        );
        Ok(Self {
            config,
            mapping,
            animation,
            staging,
        })
    }

    pub fn publish(self, graph: &Path) -> Result<Value> {
        let destination = rig_convert::animation::first_person::directory(graph);
        ensure!(
            !destination.exists(),
            "assembled graph already has an animation directory"
        );
        fs::rename(self.staging.path().join("graph/animation"), destination)
            .context("publish validated source rig")?;
        Ok(self.animation)
    }
}
