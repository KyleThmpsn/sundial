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
    /// The first reference frame's local poses, which source overlays compose onto.
    base: Vec<Pose>,
    /// Source overlays, stored composed onto [`Self::base`].
    overlays: BTreeSet<String>,
    /// Planar root distance in metres over one pass of a clip with planar root movement.
    /// Native gameplay moves the actor, so the movement itself is not played.
    strides: BTreeMap<String, f32>,
}

/// Compose a source overlay sample onto its base: rotations multiply after the base rotation
/// and translations add without rotation, as Reach applies overlays to a local pose.
fn overlay(base: Pose, delta: [f32; 8]) -> Result<Pose> {
    let [x, y, z, w, ..] = base.0;
    let [i, j, k, r, ..] = delta;
    Pose::checked([
        w * i + x * r + y * k - z * j,
        w * j - x * k + y * r + z * i,
        w * k + x * j - y * i + z * r,
        w * r - x * i - y * j - z * k,
        base.0[4] + delta[4],
        base.0[5] + delta[5],
        base.0[6] + delta[6],
        base.0[7] * delta[7],
    ])
}

/// The planar distance a clip's kind-one root movement covers, from its dx/dy frame pairs.
fn stride(root: &Path, shared: &Value, member: &Value) -> Result<f32> {
    let sizes = member["sizes"]
        .as_array()
        .context("Source motion section sizes")?;
    ensure!(sizes.len() == 17, "Source motion section layout differs");
    let offset = sizes[..4].iter().try_fold(0usize, |total, value| {
        total
            .checked_add(usize::try_from(
                value.as_u64().context("Source motion section size")?,
            )?)
            .context("Source motion section overflow")
    })?;
    let size = usize::try_from(sizes[4].as_u64().context("Source root motion size")?)?;
    let frames = usize::try_from(shared["frames"].as_u64().context("Source frames")?)?;
    ensure!(
        size == frames.checked_mul(8).context("Root motion overflow")?,
        "Source dx/dy stream does not cover its frames"
    );
    let bytes = fs::read(root.join(member["file"].as_str().context("Source motion resource")?))?;
    let pairs = bytes
        .get(offset..offset + size)
        .context("Source root motion exceeds its resource")?;
    let mut total = [0f32; 2];
    for pair in pairs.chunks_exact(8) {
        for (axis, value) in pair.chunks_exact(4).enumerate() {
            let value = f32::from_le_bytes(value.try_into().unwrap());
            ensure!(value.is_finite(), "Source root motion is not finite");
            total[axis] += value;
        }
    }
    Ok(total[0].hypot(total[1]) * crate::halo_reach::rig::METRES_PER_UNIT)
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
        let mut overlays = BTreeSet::new();
        let mut strides = BTreeMap::new();
        // Replacement clips inherit omitted channels from the selected base stance, and
        // overlays compose onto its first frame.
        let (reference, _) = Self::decode(&graph, &options.reference, root, &bind, true)?;
        let base = reference
            .first()
            .context("Empty character reference")?
            .clone();
        for name in requested {
            if name == options.reference {
                motions.insert(name.to_owned(), reference.clone());
                continue;
            }
            let kind = graph["clips"]
                .as_array()
                .context("Character clips")?
                .iter()
                .find(|clip| clip["name"] == name)
                .context("Character action missing")?["shared"][0]["animation_type"]
                .as_u64();
            let (motion, stride) = Self::decode(
                &graph,
                &name,
                root,
                if matches!(kind, Some(2 | 3)) {
                    &base
                } else {
                    &bind
                },
                false,
            )?;
            if kind == Some(2) {
                overlays.insert(name.to_owned());
            }
            if let Some(stride) = stride {
                strides.insert(name.to_owned(), stride);
            }
            motions.insert(name.to_owned(), motion);
        }
        crate::io::write_json(&root.join("character-animations.json"), &graph)?;
        Ok(Self {
            graph,
            names,
            bind,
            parents,
            motions,
            base,
            overlays,
            strides,
        })
    }

    /// Decode one clip's local poses and, for planar root movement, the distance it covers.
    /// Overlay samples compose onto `base`, and absent overlay channels leave it unchanged.
    fn decode(
        graph: &Value,
        name: &str,
        root: &Path,
        base: &[Pose],
        reference: bool,
    ) -> Result<(Vec<Vec<Pose>>, Option<f32>)> {
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
            kind == 1 || (!reference && matches!(kind, 2 | 3)),
            "Character motion requires a base, overlay or replacement action"
        );
        let planar = shared["movement_type"] == 1 && member["movement"] == 1;
        ensure!(
            planar || (shared["movement_type"] == 0 && member["movement"] == 0),
            "Character motion supports no root movement or planar root movement only"
        );
        ensure!(
            !(reference && planar),
            "A character reference stance cannot move its root"
        );
        let stride = planar.then(|| stride(root, shared, member)).transpose()?;
        let overlaid = kind == 2;
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
        let frames = (0..motion.frames)
            .map(|frame| {
                motion
                    .tracks
                    .iter()
                    .zip(base)
                    .map(|(track, &base)| {
                        let mut value = if overlaid { Pose::IDENTITY.0 } else { base.0 };
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
                        if overlaid {
                            overlay(base, value)
                        } else {
                            Pose::checked(value)
                        }
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((frames, stride))
    }

    pub fn overlay(&self, action: &Action) -> Result<bool> {
        let names = action.names(&self.graph)?;
        let overlays = names
            .iter()
            .filter(|name| self.overlays.contains(**name))
            .count();
        ensure!(
            overlays == 0 || overlays == names.len(),
            "A character action sequence mixes overlays with complete poses"
        );
        Ok(overlays > 0)
    }

    /// The object poses of the stance that overlays compose onto.
    pub fn base(&self) -> Vec<Pose> {
        self.objects(&self.base)
    }

    pub fn stride(&self, action: &Action) -> Result<Option<f32>> {
        let names = action.names(&self.graph)?;
        let [name] = names.as_slice() else {
            return Ok(None);
        };
        Ok(self.strides.get(*name).copied())
    }

    /// Sample one pass of a looping action over `frames` native frames that repeat every
    /// `period` frames. The source pass wraps from its last sample to its first.
    pub fn cycle(&self, action: &Action, frames: usize, period: usize) -> Result<Vec<Vec<Pose>>> {
        ensure!(
            frames > 0 && period > 0 && (frames == period || frames == period + 1),
            "Looping character action has an invalid native period"
        );
        let names = action.names(&self.graph)?;
        let [name] = names.as_slice() else {
            anyhow::bail!("A looping character action selects one source clip")
        };
        let samples = self
            .motions
            .get(*name)
            .context("Unprepared character action")?;
        ensure!(!samples.is_empty(), "Character action is empty");
        (0..frames)
            .map(|frame| {
                let time = frame as f32 * samples.len() as f32 / period as f32;
                let first = (time.floor() as usize).min(samples.len());
                let last = (first + 1) % samples.len();
                let locals = samples[first % samples.len()]
                    .iter()
                    .zip(&samples[last])
                    .map(|(&a, &b)| a.mix(b, time - first as f32))
                    .collect::<Result<Vec<_>>>()?;
                Ok(self.objects(&locals))
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
