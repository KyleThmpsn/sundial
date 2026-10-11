//! Portable playback on the exact source model hierarchy selected by its object branch.
use super::codec::{Motion, Track};
use crate::halo_reach::{Scene, model::Model, rig::Bone};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn mapping(graph: &Value, model: &Model) -> Result<Vec<usize>> {
    let skeleton = graph["skeleton"].as_array().context("Animation skeleton")?;
    ensure!(
        skeleton.len() == model.bones.len(),
        "The animation uses a different skeleton, requiring a complete source rig"
    );
    let names = model
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        names.len() == model.bones.len(),
        "Source model has ambiguous bone names"
    );
    let map = skeleton
        .iter()
        .map(|bone| {
            names
                .get(bone["name"].as_str().context("Animation bone name")?)
                .copied()
                .context("Animation bone is absent from the selected model")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut used = std::collections::BTreeSet::new();
    for (index, bone) in skeleton.iter().enumerate() {
        ensure!(used.insert(map[index]), "Animation bone names repeat");
        let parent = bone["parent"].as_i64().context("Animation bone parent")?;
        let parent = if parent == -1 {
            None
        } else {
            Some(
                *map.get(usize::try_from(parent)?)
                    .context("Animation parent is outside skeleton")?,
            )
        };
        ensure!(
            model.bones[map[index]].parent == parent,
            "Animation and model hierarchies differ"
        );
    }
    Ok(map)
}

fn rotation(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = a;
    let [i, j, k, r] = b;
    [
        w * i + x * r + y * k - z * j,
        w * j - x * k + y * r + z * i,
        w * k + x * j - y * i + z * r,
        w * r - x * i - y * j - z * k,
    ]
}

fn sample<T: Copy>(values: &[T], frame: usize) -> T {
    values[if values.len() == 1 { 0 } else { frame }]
}

fn rotations(track: &Track, bone: &Bone, frames: usize, overlay: bool) -> Vec<f32> {
    let mut previous = None::<[f32; 4]>;
    let mut values = Vec::with_capacity(frames * 4);
    for frame in 0..frames {
        let mut q = sample(&track.rotation, frame);
        if overlay {
            q = rotation(bone.rotation, q);
        }
        if previous.is_some_and(|p| p.iter().zip(q).map(|(a, b)| a * b).sum::<f32>() < 0.) {
            q = q.map(|v| -v);
        }
        previous = Some(q);
        values.extend(q);
    }
    values
}

struct Output {
    gltf: Value,
    bytes: Vec<u8>,
    animations: Vec<Value>,
}

impl Output {
    fn floats(
        &mut self,
        values: impl IntoIterator<Item = f32>,
        count: usize,
        kind: &str,
    ) -> Result<usize> {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let offset = self.bytes.len();
        for value in values {
            ensure!(value.is_finite(), "Nonfinite animation sample");
            self.bytes.extend(value.to_le_bytes());
        }
        let views = self.gltf["bufferViews"]
            .as_array_mut()
            .context("glTF views")?;
        let view = views.len();
        views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":self.bytes.len()-offset}));
        let accessors = self.gltf["accessors"]
            .as_array_mut()
            .context("glTF accessors")?;
        let index = accessors.len();
        accessors.push(json!({"bufferView":view,"componentType":5126,"count":count,"type":kind}));
        Ok(index)
    }

    fn clip(
        &mut self,
        motion: &Motion,
        model: &Model,
        map: &[usize],
        joints: &[Value],
        overlay: bool,
    ) -> Result<Value> {
        ensure!(
            motion.tracks.len() == map.len(),
            "Motion and source skeleton counts differ"
        );
        // Overlay scale composition is not established for this profile.
        ensure!(
            !overlay || motion.tracks.iter().all(|track| track.scale.is_empty()),
            "Overlay scale channels require a source composition rule"
        );
        let frames = motion.frames;
        let input = self.floats(
            (0..frames).map(|frame| frame as f32 / 30.),
            frames,
            "SCALAR",
        )?;
        self.gltf["accessors"][input]["min"] = json!([0.]);
        self.gltf["accessors"][input]["max"] = json!([(frames - 1) as f32 / 30.]);
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        for (source, track) in motion.tracks.iter().enumerate() {
            let index = map[source];
            let bone = &model.bones[index];
            for path in ["rotation", "translation", "scale"] {
                let output = match path {
                    "rotation" if !track.rotation.is_empty() => {
                        self.floats(rotations(track, bone, frames, overlay), frames, "VEC4")?
                    }
                    "translation" if !track.translation.is_empty() => {
                        let values = (0..frames).flat_map(|frame| {
                            let t = sample(&track.translation, frame);
                            std::array::from_fn::<_, 3, _>(|a| {
                                t[a] + if overlay { bone.translation[a] } else { 0. }
                            })
                        });
                        self.floats(values, frames, "VEC3")?
                    }
                    "scale" if !track.scale.is_empty() => {
                        let values = (0..frames).flat_map(|frame| [sample(&track.scale, frame); 3]);
                        self.floats(values, frames, "VEC3")?
                    }
                    _ => continue,
                };
                let sampler = samplers.len();
                samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
                channels
                    .push(json!({"sampler":sampler,"target":{"node":joints[index],"path":path}}));
            }
        }
        ensure!(
            !channels.is_empty(),
            "Source clip has no transform channels"
        );
        Ok(json!({"samplers":samplers,"channels":channels}))
    }
}

pub(super) fn member<'a>(graph: &'a Value, shared: &Value) -> Result<&'a Value> {
    let group = graph["groups"]
        .as_array()
        .context("Animation groups")?
        .iter()
        .find(|group| group["index"] == shared["resource_group"])
        .context("Clip resource group missing")?;
    group["members"]
        .as_array()
        .context("Animation members")?
        .iter()
        .find(|member| member["index"] == shared["resource_member"])
        .context("Clip resource member missing")
}

fn append_clips(
    output: &mut Output,
    graph: &Value,
    model: &Model,
    map: &[usize],
    joints: &[Value],
    root: &Path,
) -> Result<Vec<Value>> {
    let mut receipt = Vec::new();
    for clip in graph["clips"].as_array().context("Animation clips")? {
        for (ordinal, shared) in clip["shared"]
            .as_array()
            .context("Shared animation data")?
            .iter()
            .enumerate()
        {
            let mut row = json!({"name":clip["name"],"shared":ordinal,"model":model.tag});
            let result = (|| -> Result<Value> {
                let member = member(graph, shared)?;
                ensure!(
                    shared["movement_type"] == 0 && member["movement"] == 0,
                    "Root movement remains in the source archive"
                );
                let kind = shared["animation_type"]
                    .as_u64()
                    .context("Animation type")?;
                ensure!(matches!(kind, 1..=3), "Unsupported source animation type");
                let file = member["motion"]["file"]
                    .as_str()
                    .context("Clip has no decoded source payload")?;
                let motion: Motion = serde_json::from_slice(&fs::read(root.join(file))?)?;
                ensure!(
                    shared["frames"].as_u64() == Some(motion.frames as u64),
                    "Clip and resource frame counts differ"
                );
                let mut animation = output.clip(&motion, model, map, joints, kind == 2)?;
                animation["name"] = json!(format!(
                    "{}: {}",
                    model.tag.path,
                    clip["name"].as_str().context("Clip name")?
                ));
                animation["extras"] = json!({"source_graph":graph["tag"],"source_clip":clip["name"],"source_shared":ordinal,
                    "composition":if kind==2 {"Overlay on model bind pose"} else {"Absolute local channels with omitted bind channels retained"},
                    "sample_rate_hz":30,"limits":["30 Hz source visualization. Source state selection, root movement, layered blending and timed events are not executed."]});
                Ok(animation)
            })();
            match result {
                Ok(animation) => {
                    row["animation"] = json!(output.animations.len());
                    row["status"] = json!("Source scene playback");
                    output.animations.push(animation);
                }
                Err(error) => {
                    row["status"] = json!("Source channels preserved");
                    row["reason"] = json!(format!("{error:#}"));
                }
            }
            receipt.push(row);
        }
    }
    Ok(receipt)
}

pub(super) fn attach(scene: &Scene, graphs: &mut [Value], root: &Path) -> Result<()> {
    let mut output = Output {
        gltf: serde_json::from_slice(&fs::read(root.join("scene.gltf"))?)?,
        bytes: fs::read(root.join("scene.bin"))?,
        animations: Vec::new(),
    };
    for graph in graphs {
        let mut receipt = Vec::new();
        for (index, placed) in scene.models.iter().enumerate().filter(|(_, placed)| {
            placed
                .animation
                .as_ref()
                .is_some_and(|tag| graph["tag"]["datum"] == tag.datum)
        }) {
            match mapping(graph, &placed.model) {
                Ok(map) => {
                    let joints = output.gltf["skins"][index]["joints"].as_array().context("Source skin joints")?.clone();
                    receipt.extend(append_clips(&mut output, graph, &placed.model, &map, &joints, root)?);
                }
                Err(error) => receipt.push(json!({"model":placed.model.tag,"status":"Source channels preserved","reason":format!("{error:#}")})),
            }
        }
        graph["scene_playback"] = json!(receipt);
    }
    if !output.animations.is_empty() {
        output.gltf["animations"] = json!(output.animations);
    }
    output.gltf["buffers"][0]["byteLength"] = json!(output.bytes.len());
    fs::write(root.join("scene.bin"), output.bytes)?;
    crate::io::write_json(&root.join("scene.gltf"), &output.gltf)
}
