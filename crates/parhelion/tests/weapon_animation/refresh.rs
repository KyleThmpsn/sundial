//! Reconvert animation from original rigs while reusing an explicit prepared model input.
use super::*;

pub(super) fn animation(
    modern: &Path,
    native: &Path,
    result: &Value,
    copied: parhelion_import::GraphReference,
    item: u32,
    output: &Path,
) -> Result<parhelion_import::GraphReference> {
    ensure!(
        result["glaive"].is_null(),
        "Glaives require their complete preparation path"
    );
    let folder = Path::new(result["folder"].as_str().context("Source folder")?);
    let read = |path: &Path| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(path)?)?) };
    let source_rig = read(&folder.join("source/rig.json"))?;
    let native_rig = read(&folder.join("native/rig.json"))?;
    let manifest = copied.directory.join("asset-graph.json");
    let mut graph = read(&manifest)?;
    ensure!(
        graph["source"] == "d2_mot",
        "Animation refresh requires a modern graph"
    );
    let work = output.join("animation-refresh").join(format!("{item:08X}"));
    let mut reader = Reader::new(modern, &work.join("calibration"), true)?;
    let calibration = graph
        .pointer("/animation/first_person/rig_calibration/item")
        .filter(|v| !v.is_null())
        .map(|v| rig::inspect(&mut reader, number(v)?, true))
        .transpose()?;
    reader.finish()?;
    let mut animation =
        parhelion_import::d2_mot::rig_convert::animation::first_person::prepare_with_rig(
            modern,
            native,
            &source_rig,
            &native_rig,
            &work,
            &copied.directory,
            calibration.as_ref(),
        )?;
    ensure!(
        animation["first_person_status"] == "linked",
        "Animation refresh failed: {animation}"
    );
    if let Some(selection) = graph["animation"].get("rig_selection") {
        animation["rig_selection"] = selection.clone();
    }
    animation["converter_revision"] = parhelion_import::CONVERTER_REVISION.into();
    animation["prepared_source_results"] = result.clone();
    graph["animation"] = animation;
    fs::write(&manifest, serde_json::to_vec_pretty(&graph)?)?;
    // Copying includes every newly emitted rig, extra clip and layer payload in
    // the portable fingerprint. Other sections retain their original revision.
    parhelion_import::GraphReference::new(&copied.directory, item)?.copy_model(
        item,
        item,
        &output.join("refreshed-graphs").join(format!("{item:08X}")),
    )
}
