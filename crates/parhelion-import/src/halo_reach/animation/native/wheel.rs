//! Source wheel overlays fitted to a native phase-controlled clip envelope.
use super::{
    super::{codec::Motion, playback},
    *,
};
use std::path::PathBuf;

/// The exported source animation graph of the scene's first model, and the folder it was exported
/// to beside the graph root.
fn source_graph(scene: &Scene, root: &Path) -> Result<(PathBuf, Value)> {
    let root = root
        .parent()
        .context("Wheel motion source directory")?
        .join("source");
    let graphs: Vec<Value> = serde_json::from_slice(&fs::read(root.join("animations.json"))?)?;
    let tag = scene.models[0]
        .animation
        .as_ref()
        .context("Wheel motion graph")?;
    let mut sources = graphs
        .into_iter()
        .filter(|g| g["tag"]["datum"] == tag.datum)
        .collect::<Vec<_>>();
    ensure!(
        sources.len() == 1,
        "Wheel motion graph is absent or ambiguous"
    );
    Ok((root, sources.remove(0)))
}

/// One source wheel suspension (Reach `jmad +0x110`): the overlay it plays, the marker the ground
/// is measured below, and the ground depths in source units at the overlay's first frame, full
/// extension, and its last, full compression.
pub(in crate::halo_reach) struct Suspension {
    pub label: String,
    pub clip: String,
    pub marker: String,
    pub mass_point_offset: f32,
    pub extension: f32,
    pub compression: f32,
}

/// The scene's source suspension rows. Only local overlays translate, and a graph exported before
/// the rows were read has none.
pub(in crate::halo_reach) fn suspension(scene: &Scene, root: &Path) -> Result<Vec<Suspension>> {
    if scene.models[0].animation.is_none() {
        return Ok(Vec::new());
    }
    let (_, graph) = source_graph(scene, root)?;
    let Some(rows) = graph["vehicle_suspension"].as_array() else {
        return Ok(Vec::new());
    };
    let clips = graph["clips"].as_array().context("Wheel clips")?;
    rows.iter()
        .map(|row| {
            let text = |key: &str| -> Result<String> {
                Ok(row[key]
                    .as_str()
                    .with_context(|| format!("Source suspension {key}"))?
                    .to_owned())
            };
            let number = |key: &str| -> Result<f32> {
                let value = row[key]
                    .as_f64()
                    .with_context(|| format!("Source suspension {key}"))?
                    as f32;
                ensure!(value.is_finite(), "Invalid source suspension {key}");
                Ok(value)
            };
            ensure!(
                row["graph_index"] == -1,
                "Source suspension overlay from another graph needs its own export"
            );
            let index = usize::try_from(
                row["animation_index"]
                    .as_i64()
                    .context("Source suspension animation")?,
            )?;
            let clip = clips
                .get(index)
                .and_then(|clip| clip["name"].as_str())
                .context("Source suspension animation is outside its graph")?
                .to_owned();
            let (extension, compression) = (
                number("full_extension_ground_depth")?,
                number("full_compression_ground_depth")?,
            );
            ensure!(
                extension > compression && compression >= 0.,
                "Source suspension depths are not ordered"
            );
            Ok(Suspension {
                label: text("label")?,
                clip,
                marker: text("marker")?,
                mass_point_offset: number("mass_point_offset")?,
                extension,
                compression,
            })
        })
        .collect()
}

pub(in crate::halo_reach) fn rotations(
    scene: &Scene,
    root: &Path,
    name: &str,
    first: usize,
    frames: usize,
) -> Result<BTreeMap<usize, Vec<[f32; 8]>>> {
    let result = overlay(scene, root, name, first, frames)?;
    ensure!(!result.is_empty(), "Wheel clip has no rotation channels");
    Ok(result)
}

/// A source overlay's moving bones sampled at `frames` native frames, as model slots from
/// `first`. A static overlay, such as the large truck cab's suspension, moves none.
pub(in crate::halo_reach) fn overlay(
    scene: &Scene,
    root: &Path,
    name: &str,
    first: usize,
    frames: usize,
) -> Result<BTreeMap<usize, Vec<[f32; 8]>>> {
    let (root, graph) = source_graph(scene, root)?;
    let graph = &graph;
    let map = playback::mapping(graph, &scene.models[0].model)?;
    let clips = graph["clips"]
        .as_array()
        .context("Wheel clips")?
        .iter()
        .filter(|c| c["name"] == name)
        .collect::<Vec<_>>();
    ensure!(clips.len() == 1, "Wheel clip {name} is absent or ambiguous");
    let variants = clips[0]["shared"]
        .as_array()
        .context("Wheel clip variants")?;
    ensure!(
        variants.len() == 1,
        "Wheel clip variants need an explicit selection"
    );
    let variant = &variants[0];
    let member = playback::member(graph, variant)?;
    ensure!(
        variant["animation_type"] == 2 && variant["movement_type"] == 0 && member["movement"] == 0,
        "Wheel motion is not an in-place source overlay"
    );
    let file = member["motion"]["file"]
        .as_str()
        .context("Wheel motion data")?;
    let motion: Motion = serde_json::from_slice(&fs::read(root.join(file))?)?;
    ensure!(
        motion.frames >= 2 && frames >= 2 && motion.tracks.len() == map.len(),
        "Wheel motion dimensions differ"
    );
    let mut result = BTreeMap::new();
    for (index, track) in motion.tracks.iter().enumerate() {
        let bone = &scene.models[0].model.bones[map[index]];
        // Overlay translation adds to the bind translation, as in source playback.
        // Overlay scale composition is not established, so only unit scale passes.
        ensure!(
            track
                .scale
                .iter()
                .all(|&scale| (scale - 1.).abs() < 0.00001),
            "Wheel scale overlays need a separate composition rule"
        );
        ensure!(
            track.translation.is_empty()
                || track.translation.len() == motion.frames
                || track.translation.len() == 1,
            "Incomplete wheel translation channel"
        );
        if track.rotation.is_empty() && track.translation.iter().all(|t| *t == [0.; 3]) {
            continue;
        }
        ensure!(
            track.rotation.len() == motion.frames || track.rotation.len() <= 1,
            "Incomplete wheel rotation channel"
        );
        // The source frame span sampled at native frame `frame`, as two indices and a blend.
        let span = |len: usize, frame: usize| {
            let time = frame as f32 * len.saturating_sub(1) as f32 / (frames - 1) as f32;
            let lo = time.floor() as usize;
            (lo, (lo + 1).min(len.saturating_sub(1)), time - lo as f32)
        };
        let mut values = Vec::with_capacity(frames);
        for frame in 0..frames {
            let mut q = [0., 0., 0., 1.];
            if !track.rotation.is_empty() {
                let (lo, hi, t) = span(track.rotation.len(), frame);
                let a = track.rotation[lo];
                let mut b = track.rotation[hi];
                if a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>() < 0. {
                    b.iter_mut().for_each(|v| *v = -*v);
                }
                q = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
                let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt();
                ensure!(
                    norm.is_finite() && norm > 0.000001,
                    "Degenerate wheel rotation"
                );
                q.iter_mut().for_each(|v| *v /= norm);
            }
            let q = multiply(bone.rotation, q);
            let mut position = bone.translation;
            if !track.translation.is_empty() {
                let (lo, hi, t) = span(track.translation.len(), frame);
                let (a, b) = (track.translation[lo], track.translation[hi]);
                for axis in 0..3 {
                    position[axis] += a[axis] + (b[axis] - a[axis]) * t;
                }
            }
            values.push([
                q[0],
                q[1],
                q[2],
                q[3],
                position[0],
                position[1],
                position[2],
                1.,
            ]);
        }
        result.insert(first + map[index], values);
    }
    Ok(result)
}
