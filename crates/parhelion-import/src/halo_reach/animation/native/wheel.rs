//! Source wheel overlays fitted to a native phase-controlled clip envelope.
use super::{
    super::{codec::Motion, playback},
    *,
};

pub(in crate::halo_reach) fn rotations(
    scene: &Scene,
    root: &Path,
    name: &str,
    first: usize,
    frames: usize,
) -> Result<BTreeMap<usize, Vec<[f32; 8]>>> {
    let root = root
        .parent()
        .context("Wheel motion source directory")?
        .join("source");
    let graphs: Vec<Value> = serde_json::from_slice(&fs::read(root.join("animations.json"))?)?;
    let tag = scene.models[0]
        .animation
        .as_ref()
        .context("Wheel motion graph")?;
    let sources = graphs
        .iter()
        .filter(|g| g["tag"]["datum"] == tag.datum)
        .collect::<Vec<_>>();
    ensure!(
        sources.len() == 1,
        "Wheel motion graph is absent or ambiguous"
    );
    let graph = sources[0];
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
        ensure!(
            track.translation.is_empty() && track.scale.is_empty(),
            "Wheel translation or scale overlays need a separate composition rule"
        );
        if track.rotation.is_empty() {
            continue;
        }
        ensure!(
            track.rotation.len() == motion.frames || track.rotation.len() == 1,
            "Incomplete wheel rotation channel"
        );
        let mut values = Vec::with_capacity(frames);
        for frame in 0..frames {
            let time = frame as f32 * (track.rotation.len() - 1) as f32 / (frames - 1) as f32;
            let lo = time.floor() as usize;
            let hi = (lo + 1).min(track.rotation.len() - 1);
            let a = track.rotation[lo];
            let mut b = track.rotation[hi];
            if a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>() < 0. {
                b.iter_mut().for_each(|v| *v = -*v);
            }
            let t = time - lo as f32;
            let mut q: [f32; 4] = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
            let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt();
            ensure!(
                norm.is_finite() && norm > 0.000001,
                "Degenerate wheel rotation"
            );
            q.iter_mut().for_each(|v| *v /= norm);
            let q = multiply(bone.rotation, q);
            values.push([
                q[0],
                q[1],
                q[2],
                q[3],
                bone.translation[0],
                bone.translation[1],
                bone.translation[2],
                1.,
            ]);
        }
        result.insert(first + map[index], values);
    }
    ensure!(!result.is_empty(), "Wheel clip has no rotation channels");
    Ok(result)
}
