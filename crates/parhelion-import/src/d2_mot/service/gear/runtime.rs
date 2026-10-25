//! Prepare equipment runtime assets before pinning and sharing the complete item.
use super::*;

pub(super) fn prepare(
    paths: &SourcePaths<'_>,
    source: &Value,
    native: &Value,
    work: &Path,
    output: &Path,
    graph: &mut Value,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    crate::cancellation::check()?;
    if source["runtime_kind"] != "equipment" {
        let detail = if graph["rendering"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                part["cloth"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row["simulation_converted"] == true))
            })
        }) {
            "Armor follows native character animation. Converted cloth uses native simulation. Its motion still needs an in-game check."
        } else {
            "Armor follows native character animation."
        };
        graph["limitations"]
            .as_array_mut()
            .context("Import details")?
            .push(json!(detail));
        return Ok(());
    }
    progress("Converting equipment animation clips...".into());
    let animation = crate::d2_mot::rig_convert::animation::equipment::prepare(
        paths.modern,
        paths.native,
        source,
        native,
        &work.join("animation"),
        output,
    );
    crate::cancellation::check()?;
    let animation = match animation {
        Ok(animation) => animation,
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(error) => {
            json!({"status":"native","reason":format!("{error:#}"),"gameplay_verified":false})
        }
    };
    let mut details = Vec::new();
    if animation["status"] == "linked" {
        let skipped = animation["unconverted"].as_array().map_or(0, Vec::len);
        let source_only = animation["source_only"].as_array().map_or(0, Vec::len);
        if skipped > 0 || source_only > 0 {
            details.push(json!(format!("Equipment animation keeps {skipped} native clips and has {source_only} source clips without a matching native trigger.")));
        }
    } else {
        details.push(json!(format!(
            "Equipment animation remains native. {}",
            animation["reason"]
                .as_str()
                .unwrap_or("No compatible animation lookup")
        )));
    }
    graph["equipment_animation"] = animation;
    if source["content_key"].is_string() && native["content_key"].is_string() {
        for (label, rig) in [("Source", source), ("Native", native)] {
            if rig["audio"]["status"] == "unavailable" {
                details.push(json!(format!(
                    "{label} equipment audio could not be read. {}",
                    rig["audio"]["reason"]
                        .as_str()
                        .unwrap_or("Unknown audio layout")
                )));
            }
        }
        progress("Converting matched equipment audio...".into());
        crate::cancellation::check()?;
        let mut audio =
            crate::d2_mot::audio::prepare(paths.modern, paths.native, output, source, native);
        crate::cancellation::check()?;
        if let Some(errors) = audio["conversion_errors"]
            .as_array()
            .filter(|e| !e.is_empty())
        {
            details.push(json!(format!(
                "Equipment audio remains native. {}",
                errors
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(". ")
            )));
            // Keep the diagnostic, but never expose an incomplete conversion as an authorable bank.
            audio["authoring_schema"] = Value::Null;
            audio["source_media_imported"] = json!(false);
        }
        let unmatched = ["unmatched_events", "unmatched_firing_events"]
            .iter()
            .map(|key| audio[*key].as_array().map_or(0, Vec::len))
            .sum::<usize>();
        if unmatched > 0 {
            details.push(json!(format!(
                "{unmatched} source audio cues have no matching native trigger."
            )));
        }
        graph["audio"] = audio;
    }
    graph["limitations"]
        .as_array_mut()
        .context("Import details")?
        .extend(details);
    Ok(())
}
