//! Reconstruct absolute source arm poses from bind poses and decoded channels.
use super::*;
use crate::{halo_reach::rig::Bone, tiger::animation::pose::Pose};
pub(super) struct Source {
    pub graph: Value,
    pub names: BTreeMap<String, usize>,
    parents: Vec<Option<usize>>,
    bind: Vec<Pose>,
}

fn bind(bone: &Bone) -> Result<Pose> {
    Pose::checked([
        bone.rotation[0],
        bone.rotation[1],
        bone.rotation[2],
        bone.rotation[3],
        bone.translation[0],
        bone.translation[1],
        bone.translation[2],
        1.,
    ])
}

impl Source {
    pub fn read(cache: &Cache, scene: &Scene, options: &Options, root: &Path) -> Result<Self> {
        let arm_tag = cache.find("mode", &options.arms_model)?;
        let arms = crate::halo_reach::model::read_bones(cache, arm_tag.address()?)?;
        let graphs: Vec<Value> = serde_json::from_slice(&fs::read(root.join("animations.json"))?)?;
        let mut candidates = Vec::new();
        for branch in &scene.object.first_person {
            let (Some(model), Some(animation)) = (&branch.model, &branch.animation) else {
                continue;
            };
            let weapon = crate::halo_reach::model::read_bones(cache, model.address()?)?;
            let Some(graph) = graphs.iter().find(|g| g["tag"]["datum"] == animation.datum) else {
                continue;
            };
            if let Some(source) = Self::bind_graph(graph, &arms, &weapon)? {
                candidates.push(source);
            }
        }
        ensure!(
            candidates.len() == 1,
            "Source first-person branch does not uniquely match the configured arm rig"
        );
        Ok(candidates.remove(0))
    }

    fn bind_graph(graph: &Value, arms: &[Bone], weapon: &[Bone]) -> Result<Option<Self>> {
        let mut bindings = BTreeMap::new();
        for bone in arms.iter().chain(weapon) {
            ensure!(
                bindings.insert(bone.name.clone(), bone).is_none(),
                "Source arm and weapon bone names overlap"
            );
        }
        let bones = graph["skeleton"]
            .as_array()
            .context("Source arm skeleton")?;
        if bones.len() != bindings.len()
            || bones.iter().any(|b| {
                b["name"]
                    .as_str()
                    .is_none_or(|name| !bindings.contains_key(name))
            })
        {
            return Ok(None);
        }
        let mut names = BTreeMap::new();
        let mut poses = Vec::new();
        let mut parents = Vec::new();
        for (i, bone) in bones.iter().enumerate() {
            let name = bone["name"].as_str().context("Source arm bone name")?;
            ensure!(
                names.insert(name.to_owned(), i).is_none(),
                "Repeated source arm bone"
            );
            let parent = bone["parent"].as_i64().context("Source arm parent")?;
            ensure!(
                parent >= -1 && parent < i as i64,
                "Source arm rig is not parent-first"
            );
            let source = bindings[name];
            let model_bones = if arms.iter().any(|b| b.name == name) {
                arms
            } else {
                weapon
            };
            let expected = source.parent.map(|p| model_bones[p].name.as_str());
            let actual = if parent < 0 {
                None
            } else {
                bones[parent as usize]["name"].as_str()
            };
            ensure!(
                expected == actual
                    || (name == "b_gun" && expected.is_none() && actual == Some("r_hand")),
                "Source arm and model parents disagree for {name}"
            );
            parents.push((parent >= 0).then_some(parent as usize));
            poses.push(bind(source)?);
        }
        Ok(Some(Self {
            graph: graph.clone(),
            names,
            bind: poses,
            parents,
        }))
    }

    fn channels(&self, motion: &super::super::codec::Motion) -> Result<Vec<Vec<Pose>>> {
        let mut channels = Vec::new();
        for (track, &bind) in motion.tracks.iter().zip(&self.bind) {
            ensure!(
                [
                    track.rotation.len(),
                    track.translation.len(),
                    track.scale.len()
                ]
                .iter()
                .all(|&n| n == 0 || n == 1 || n == motion.frames),
                "Incomplete source arm channel"
            );
            channels.push(
                (0..motion.frames)
                    .map(|frame| sample(track, bind, frame))
                    .collect::<Result<_>>()?,
            );
        }
        Ok(channels)
    }

    pub fn clip(&self, root: &Path, name: &str, frames: usize) -> Result<Vec<Vec<Pose>>> {
        let clips = self.graph["clips"]
            .as_array()
            .context("Source arm clips")?
            .iter()
            .filter(|c| c["name"] == name)
            .collect::<Vec<_>>();
        ensure!(
            clips.len() == 1,
            "Source arm clip {name} is absent or ambiguous"
        );
        let variants = clips[0]["shared"]
            .as_array()
            .context("Source arm clip variants")?;
        ensure!(
            variants.len() == 1,
            "Source arm clip needs an explicit variant selection"
        );
        let shared = &variants[0];
        let member = super::super::playback::member(&self.graph, shared)?;
        ensure!(
            shared["animation_type"] == 1
                && shared["movement_type"] == 0
                && member["movement"] == 0,
            "Arm translation requires absolute source motion without root movement"
        );
        let file = member["motion"]["file"]
            .as_str()
            .context("Source arm motion file")?;
        let motion: super::super::codec::Motion =
            serde_json::from_slice(&fs::read(root.join(file))?)?;
        ensure!(
            motion.frames > 1 && frames > 1 && motion.tracks.len() == self.bind.len(),
            "Source arm motion dimensions disagree"
        );
        let channels = self.channels(&motion)?;
        let mut output = Vec::new();
        for frame in 0..frames {
            let t = frame as f32 * (motion.frames - 1) as f32 / (frames - 1) as f32;
            let low = t.floor() as usize;
            let high = (low + 1).min(motion.frames - 1);
            let mut objects = Vec::<Pose>::new();
            for (i, values) in channels.iter().enumerate() {
                let local = values[low].mix(values[high], t - low as f32)?;
                objects.push(self.parents[i].map_or(local, |p| objects[p].then(local)));
            }
            output.push(objects);
        }
        Ok(output)
    }
}

fn sample(track: &super::super::codec::Track, bind: Pose, frame: usize) -> Result<Pose> {
    let mut v = bind.0;
    if let Some(q) = track
        .rotation
        .get(if track.rotation.len() == 1 { 0 } else { frame })
    {
        v[..4].copy_from_slice(q);
    }
    if let Some(p) = track.translation.get(if track.translation.len() == 1 {
        0
    } else {
        frame
    }) {
        v[4..7].copy_from_slice(p);
    }
    if let Some(s) = track
        .scale
        .get(if track.scale.len() == 1 { 0 } else { frame })
    {
        v[7] = *s;
    }
    Pose::checked(v)
}
