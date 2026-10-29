//! Decode selected character actions on their complete, name-checked source rig.
use super::{Action, Options};
use crate::{
    halo_reach::{cache::Cache, model, resource::Pages},
    tiger::animation::pose::Pose,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(super) struct Source {
    pub graph: Value,
    pub names: BTreeMap<String, usize>,
    pub bind: Vec<Pose>,
    pub parents: Vec<Option<usize>>,
    motions: BTreeMap<String, Vec<Vec<Pose>>>,
}

impl Source {
    pub fn read(cache: &Cache, pages: &mut Pages, options: &Options, root: &Path) -> Result<Self> {
        let graph_tag = cache.find("jmad", &options.graph)?;
        let model_tag = cache.find("mode", &options.model)?;
        let bones = model::read_bones(cache, model_tag.address()?)?;
        let routes = json!({"routes":super::super::route::read(cache, graph_tag.address()?)?});
        let mut requested = BTreeSet::from([options.reference.clone()]);
        for action in options.motions() {
            requested.extend(action.names(&routes)?.into_iter().map(str::to_owned));
        }
        ensure!(
            options.motions().next().is_some(),
            "No source character actions selected"
        );
        let mut data = crate::halo_reach::behavior::animation_data(cache, graph_tag)?;
        let clips = data["clips"]
            .as_array_mut()
            .context("Source character clips")?;
        clips.retain(|clip| {
            clip["name"]
                .as_str()
                .is_some_and(|name| requested.contains(name))
        });
        let mut groups = BTreeSet::new();
        let mut found = BTreeSet::new();
        for clip in clips {
            let name = clip["name"]
                .as_str()
                .context("Source character clip name")?;
            ensure!(
                found.insert(name.to_owned()),
                "Repeated source character action {name}"
            );
            for variant in clip["shared"]
                .as_array()
                .context("Character motion variants")?
            {
                groups.insert(usize::try_from(
                    variant["resource_group"]
                        .as_u64()
                        .context("Character motion group")?,
                )?);
            }
        }
        ensure!(
            found.len() == requested.len(),
            "A requested character action is absent from its source graph"
        );
        let graph = super::super::export_graph(
            cache,
            pages,
            &json!({"tag":graph_tag,"data":data}),
            Some(&groups),
            root,
        )?;
        ensure!(
            graph["clips"]
                .as_array()
                .context("Resolved character clips")?
                .iter()
                .all(|clip| clip["shared"].as_array().is_some_and(|v| v.len() == 1)),
            "Character action requires explicit variant selection"
        );
        let skeleton = graph["skeleton"]
            .as_array()
            .context("Source character skeleton")?;
        let by_name = bones
            .iter()
            .map(|bone| (bone.name.as_str(), bone))
            .collect::<BTreeMap<_, _>>();
        ensure!(
            by_name.len() == bones.len() && skeleton.len() == bones.len(),
            "Character model and graph bone counts disagree"
        );
        let mut names = BTreeMap::new();
        let mut bind = Vec::new();
        let mut parents = Vec::new();
        for (index, node) in skeleton.iter().enumerate() {
            let name = node["name"].as_str().context("Character bone name")?;
            let bone = by_name
                .get(name)
                .context("Character graph bone is absent from its model")?;
            ensure!(
                names.insert(name.to_owned(), index).is_none(),
                "Repeated character graph bone"
            );
            let parent = node["parent"].as_i64().context("Character graph parent")?;
            ensure!(
                parent >= -1 && parent < index as i64,
                "Character graph is not parent-first"
            );
            let parent = (parent >= 0).then_some(parent as usize);
            ensure!(
                bone.parent.map(|p| bones[p].name.as_str())
                    == parent.and_then(|p| skeleton[p]["name"].as_str()),
                "Character model and graph ancestry disagree for {name}"
            );
            parents.push(parent);
            bind.push(Pose::checked([
                bone.rotation[0],
                bone.rotation[1],
                bone.rotation[2],
                bone.rotation[3],
                bone.translation[0],
                bone.translation[1],
                bone.translation[2],
                1.,
            ])?);
        }
        let mut motions = BTreeMap::new();
        // Replacement clips inherit omitted channels from the selected base stance.
        let reference = Self::decode(&graph, &options.reference, root, &bind, true)?;
        let base = reference.first().context("Empty character reference")?;
        for name in requested {
            motions.insert(
                name.to_owned(),
                if name == options.reference {
                    reference.clone()
                } else {
                    let replacement = graph["clips"]
                        .as_array()
                        .context("Character clips")?
                        .iter()
                        .find(|clip| clip["name"] == name)
                        .context("Character action missing")?["shared"][0]["animation_type"]
                        == 3;
                    Self::decode(
                        &graph,
                        &name,
                        root,
                        if replacement { base } else { &bind },
                        false,
                    )?
                },
            );
        }
        crate::io::write_json(&root.join("character-animations.json"), &graph)?;
        Ok(Self {
            graph,
            names,
            bind,
            parents,
            motions,
        })
    }

    fn decode(
        graph: &Value,
        name: &str,
        root: &Path,
        base: &[Pose],
        reference: bool,
    ) -> Result<Vec<Vec<Pose>>> {
        let clips = graph["clips"].as_array().context("Character clips")?;
        let clip = clips
            .iter()
            .find(|c| c["name"] == name)
            .context("Character action missing")?;
        let shared = &clip["shared"][0];
        let member = super::super::playback::member(graph, shared)?;
        let kind = shared["animation_type"]
            .as_u64()
            .context("Character action kind")?;
        ensure!(
            (kind == 1 || (!reference && kind == 3))
                && shared["movement_type"] == 0
                && member["movement"] == 0,
            "Character motion requires a base or replacement action without root movement"
        );
        let file = member["motion"]["file"]
            .as_str()
            .context("Decoded character motion missing")?;
        let motion: super::super::codec::Motion =
            serde_json::from_slice(&fs::read(root.join(file))?)?;
        ensure!(
            motion.frames > 0
                && motion.tracks.len() == base.len()
                && shared["frames"].as_u64() == Some(motion.frames as u64),
            "Character motion dimensions disagree"
        );
        for track in &motion.tracks {
            ensure!(
                [
                    track.rotation.len(),
                    track.translation.len(),
                    track.scale.len()
                ]
                .iter()
                .all(|&length| length == 0 || length == 1 || length == motion.frames),
                "Incomplete character motion channel"
            );
        }
        (0..motion.frames)
            .map(|frame| {
                motion
                    .tracks
                    .iter()
                    .zip(base)
                    .map(|(track, &base)| {
                        let mut value = base.0;
                        if let Some(q) =
                            track
                                .rotation
                                .get(if track.rotation.len() == 1 { 0 } else { frame })
                        {
                            value[..4].copy_from_slice(q);
                        }
                        if let Some(p) = track.translation.get(if track.translation.len() == 1 {
                            0
                        } else {
                            frame
                        }) {
                            value[4..7].copy_from_slice(p);
                        }
                        if let Some(scale) =
                            track
                                .scale
                                .get(if track.scale.len() == 1 { 0 } else { frame })
                        {
                            value[7] = *scale;
                        }
                        ensure!(
                            (value[7] - 1.).abs() < 1e-4,
                            "Character motion contains unsupported bone scaling"
                        );
                        Pose::checked(value)
                    })
                    .collect()
            })
            .collect()
    }

    pub fn objects(&self, locals: &[Pose]) -> Vec<Pose> {
        let mut output: Vec<Pose> = Vec::with_capacity(locals.len());
        for (index, &local) in locals.iter().enumerate() {
            output.push(self.parents[index].map_or(local, |p| output[p].then(local)));
        }
        output
    }

    pub fn action(
        &self,
        action: &Action,
        frames: usize,
        contact: Option<(usize, usize)>,
    ) -> Result<Vec<Vec<Pose>>> {
        ensure!(frames > 0, "Character action has no output frames");
        let names = action.names(&self.graph)?;
        let mut samples = Vec::new();
        for name in names {
            samples.extend(
                self.motions
                    .get(name)
                    .context("Unprepared character action")?
                    .iter(),
            );
        }
        ensure!(!samples.is_empty(), "Character action is empty");
        if let Some((source, native)) = contact {
            ensure!(
                source > 0 && source < samples.len() - 1 && native > 0 && native < frames - 1,
                "Character contact is outside its source or native interval"
            );
        }
        (0..frames)
            .map(|frame| {
                let time = if frames == 1 {
                    0.
                } else if let Some((source, native)) = contact {
                    // Retain both endpoints and place the source primary pose on
                    // the existing contact event, without moving native events.
                    if frame <= native {
                        frame as f32 * source as f32 / native as f32
                    } else {
                        source as f32
                            + (frame - native) as f32 * (samples.len() - 1 - source) as f32
                                / (frames - 1 - native) as f32
                    }
                } else {
                    frame as f32 * (samples.len() - 1) as f32 / (frames - 1) as f32
                };
                let first = time.floor() as usize;
                let last = (first + 1).min(samples.len() - 1);
                let locals = samples[first]
                    .iter()
                    .zip(samples[last])
                    .map(|(&a, &b)| a.mix(b, time - first as f32))
                    .collect::<Result<Vec<_>>>()?;
                Ok(self.objects(&locals))
            })
            .collect()
    }

    pub fn frames(&self, action: &Action) -> Result<usize> {
        let names = action.names(&self.graph)?;
        names.iter().try_fold(0usize, |count, name| {
            count
                .checked_add(
                    self.motions
                        .get(*name)
                        .context("Unprepared character action")?
                        .len(),
                )
                .context("Character action frame count overflows")
        })
    }
}
