//! Reconstruct absolute source arm poses from bind poses and decoded channels.
use super::*;
use crate::{halo_reach::rig::Bone, tiger::animation::pose::Pose};
pub(super) struct Source {
    pub graph: Value,
    pub names: BTreeMap<String, usize>,
    pub weapon: BTreeSet<String>,
    pub root: usize,
    parents: Vec<Option<usize>>,
    bind: Vec<Pose>,
    tracks: Vec<usize>,
    scale: f32,
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

fn stationary(root: &Path, shared: &Value, member: &Value) -> Result<bool> {
    if shared["movement_type"] == 0 && member["movement"] == 0 {
        return Ok(true);
    }
    if shared["movement_type"] != 1 || member["movement"] != 1 {
        return Ok(false);
    }
    // A declared dx/dy stream can contain no displacement. Keep the original
    // root-motion boundary for nonzero values, including nonfinite samples.
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
    let frames = usize::try_from(
        shared["frames"]
            .as_u64()
            .context("Source root motion frames")?,
    )?;
    ensure!(
        size == frames
            .checked_mul(8)
            .context("Source root motion size overflow")?,
        "Source dx/dy stream does not cover its frames"
    );
    let bytes = fs::read(root.join(member["file"].as_str().context("Source motion resource")?))?;
    let end = offset
        .checked_add(size)
        .context("Source root motion extent overflow")?;
    Ok(bytes
        .get(offset..end)
        .context("Source root motion exceeds its resource")?
        .chunks_exact(4)
        .all(|v| f32::from_le_bytes(v.try_into().unwrap()) == 0.))
}

impl Source {
    pub fn bind_objects(&self) -> Vec<Pose> {
        self.objects(&self.bind)
    }

    fn objects(&self, locals: &[Pose]) -> Vec<Pose> {
        let mut objects: Vec<Pose> = Vec::with_capacity(locals.len());
        for (index, &local) in locals.iter().enumerate() {
            objects.push(self.parents[index].map_or(local, |parent| objects[parent].then(local)));
        }
        objects
    }

    pub fn read(cache: &Cache, scene: &Scene, options: &Options, root: &Path) -> Result<Self> {
        Self::select(cache, scene, &options.arms_model, root)?
            .context("Source first-person branch does not match the configured arm rig")
    }

    pub(super) fn select(
        cache: &Cache,
        scene: &Scene,
        arms_model: &str,
        root: &Path,
    ) -> Result<Option<Self>> {
        let arm_tag = cache.find("mode", arms_model)?;
        let mut arms = crate::halo_reach::model::read_bones(cache, arm_tag.address()?)?;
        crate::halo_reach::native::scale::bones(&mut arms, scene.scale);
        let graphs: Vec<Value> = serde_json::from_slice(&fs::read(root.join("animations.json"))?)?;
        let mut candidates = Vec::new();
        for branch in &scene.object.first_person {
            let (Some(model), Some(animation)) = (&branch.model, &branch.animation) else {
                continue;
            };
            let mut weapon = crate::halo_reach::model::read_bones(cache, model.address()?)?;
            crate::halo_reach::native::scale::bones(&mut weapon, scene.scale);
            let Some(graph) = graphs.iter().find(|g| g["tag"]["datum"] == animation.datum) else {
                continue;
            };
            if let Some(mut source) = Self::bind_graph(graph, &arms, &weapon)? {
                source.scale = scene.scale;
                candidates.push(source);
            }
        }
        ensure!(
            candidates.len() <= 1,
            "Source first-person branch does not uniquely match the configured arm rig"
        );
        Ok(candidates.pop())
    }

    pub(super) fn supports(&self, root: &Path, name: &str) -> Result<bool> {
        let clips = self.graph["clips"].as_array().context("Source arm clips")?;
        let candidates = clips
            .iter()
            .filter(|clip| clip["name"] == name)
            .collect::<Vec<_>>();
        ensure!(candidates.len() <= 1, "Repeated source arm clip {name}");
        let Some(clip) = candidates.first() else {
            return Ok(false);
        };
        let variants = clip["shared"]
            .as_array()
            .context("Source arm clip variants")?;
        let [shared] = variants.as_slice() else {
            return Ok(false);
        };
        if shared["animation_type"] != 1 {
            return Ok(false);
        }
        let member = super::super::playback::member(&self.graph, shared)?;
        stationary(root, shared, member)
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
        let graph_names = bones
            .iter()
            .map(|bone| bone["name"].as_str().context("Source arm bone name"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(graph_names.len() == bones.len(), "Repeated source arm bone");
        if bindings
            .keys()
            .any(|name| !graph_names.contains(name.as_str()))
        {
            return Ok(None);
        }
        let mut names = BTreeMap::new();
        let mut poses = Vec::new();
        let mut parents = Vec::new();
        let mut tracks = Vec::new();
        for (i, bone) in bones.iter().enumerate() {
            let name = bone["name"].as_str().context("Source arm bone name")?;
            let parent = bone["parent"].as_i64().context("Source arm parent")?;
            ensure!(
                parent >= -1 && parent < i as i64,
                "Source arm rig is not parent-first"
            );
            let Some(&source) = bindings.get(name) else {
                // Auxiliary animation joints may have no model geometry. They
                // are excluded only when no retained bone depends on them.
                continue;
            };
            let arm = arms.iter().any(|b| b.name == name);
            let model_bones = if arm { arms } else { weapon };
            let expected = source.parent.map(|p| model_bones[p].name.as_str());
            let actual = if parent < 0 {
                None
            } else {
                bones[parent as usize]["name"].as_str()
            };
            // The combined animation graph attaches a standalone weapon root
            // to an arm bone. Root names vary between weapon families. Internal
            // weapon joints and the arm hierarchy must still match their models.
            let attachment = !arm
                && expected.is_none()
                && actual.is_some_and(|parent| arms.iter().any(|bone| bone.name == parent));
            ensure!(
                expected == actual || attachment,
                "Source arm and model parents disagree for {name}"
            );
            parents.push(
                actual
                    .map(|name| {
                        names
                            .get(name)
                            .copied()
                            .context("Source arm parent is not bound")
                    })
                    .transpose()?,
            );
            names.insert(name.to_owned(), poses.len());
            poses.push(bind(source)?);
            tracks.push(i);
        }
        let mut roots = weapon.iter().filter(|bone| bone.parent.is_none());
        let root = roots.next().context("Source weapon root missing")?;
        ensure!(roots.next().is_none(), "Source weapon root is ambiguous");
        let root = *names
            .get(&root.name)
            .context("Source weapon root is not bound")?;
        Ok(Some(Self {
            graph: graph.clone(),
            names,
            weapon: weapon.iter().map(|bone| bone.name.clone()).collect(),
            root,
            bind: poses,
            parents,
            tracks,
            scale: 1.,
        }))
    }

    pub fn weapon_role<'a>(&self, name: &'a str) -> &'a str {
        if self.names.get(name) == Some(&self.root) {
            "b_handle"
        } else {
            match name {
                "b_gun" | "b_weapon" => "b_handle",
                "b_mag" => "b_magazine",
                other => other,
            }
        }
    }

    fn channels(&self, motion: &super::super::codec::Motion) -> Result<Vec<Vec<Pose>>> {
        let mut channels = Vec::new();
        for (&index, &bind) in self.tracks.iter().zip(&self.bind) {
            let track = motion
                .tracks
                .get(index)
                .context("Source arm motion track")?;
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
                    .map(|frame| sample(track, bind, frame, self.scale))
                    .collect::<Result<_>>()?,
            );
        }
        Ok(channels)
    }

    pub fn action(&self, root: &Path, action: &Action, frames: usize) -> Result<Vec<Vec<Pose>>> {
        let names = action.names(&self.graph)?;
        if !matches!(action, Action::Sequence(_)) {
            return self.clip(root, names[0], frames);
        }
        ensure!(
            !names.is_empty() && names.len() <= 16 && frames > 0,
            "Invalid source action sequence"
        );
        let clips = self.graph["clips"].as_array().context("Source arm clips")?;
        let mut sequence = Vec::new();
        for name in names {
            let clip = clips
                .iter()
                .find(|c| c["name"] == name)
                .context("Source sequence clip")?;
            let count = usize::try_from(
                clip["shared"][0]["frames"]
                    .as_u64()
                    .context("Source sequence duration")?,
            )?;
            sequence.extend(self.locals(root, name, count)?);
        }
        ensure!(!sequence.is_empty(), "Source action sequence is empty");
        (0..frames)
            .map(|frame| {
                let time = if frames == 1 {
                    0.
                } else {
                    frame as f32 * (sequence.len() - 1) as f32 / (frames - 1) as f32
                };
                let low = time.floor() as usize;
                let high = (low + 1).min(sequence.len() - 1);
                // Interpolate before hierarchy composition so a rotating parent does not
                // shorten its children's offsets between source frames or clip boundaries.
                let locals = sequence[low]
                    .iter()
                    .zip(&sequence[high])
                    .map(|(&a, &b)| a.mix(b, time - low as f32))
                    .collect::<Result<Vec<_>>>()?;
                Ok(self.objects(&locals))
            })
            .collect()
    }

    pub fn clip(&self, root: &Path, name: &str, frames: usize) -> Result<Vec<Vec<Pose>>> {
        Ok(self
            .locals(root, name, frames)?
            .iter()
            .map(|locals| self.objects(locals))
            .collect())
    }

    fn locals(&self, root: &Path, name: &str, frames: usize) -> Result<Vec<Vec<Pose>>> {
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
            shared["animation_type"] == 1 && stationary(root, shared, member)?,
            "Arm translation requires absolute source motion without root movement: {name}"
        );
        let file = member["motion"]["file"]
            .as_str()
            .context("Source arm motion file")?;
        let motion: super::super::codec::Motion =
            serde_json::from_slice(&fs::read(root.join(file))?)?;
        ensure!(
            motion.frames > 0
                && frames > 0
                && Some(motion.tracks.len()) == self.graph["skeleton"].as_array().map(Vec::len)
                && shared["frames"].as_u64() == Some(motion.frames as u64),
            "Source arm motion dimensions disagree"
        );
        let channels = self.channels(&motion)?;
        let mut output = Vec::new();
        for frame in 0..frames {
            let t = if frames == 1 {
                0.
            } else {
                frame as f32 * (motion.frames - 1) as f32 / (frames - 1) as f32
            };
            let low = t.floor() as usize;
            let high = (low + 1).min(motion.frames - 1);
            output.push(
                channels
                    .iter()
                    .map(|values| values[low].mix(values[high], t - low as f32))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        Ok(output)
    }
}

fn sample(
    track: &super::super::codec::Track,
    bind: Pose,
    frame: usize,
    scale: f32,
) -> Result<Pose> {
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
        v[4..7].copy_from_slice(&p.map(|v| v * scale));
    }
    if let Some(s) = track
        .scale
        .get(if track.scale.len() == 1 { 0 } else { frame })
    {
        v[7] = *s;
    }
    Pose::checked(v)
}
