//! Private clips joined to a shared native human bank. Character animations and imported human
//! actions both add clips after every existing descriptor of a stock bank. The clips take one
//! asset group, their descriptors extend the bank, and the group joins the runtime dependency
//! root. Stock descriptors, clip rows and names keep their ordinals.
use super::*;
use parhelion_import::tiger::{animation, payload::Payload};

/// The class of the native clips a prepared clip copies.
const CLIP_CLASS: u32 = 0x8080_8F49;

fn word(value: &Value, key: &str) -> anyhow::Result<u32> {
    use anyhow::Context;
    Ok(u32::try_from(value[key].as_u64().with_context(|| {
        format!("Prepared clip {key} missing")
    })?)?)
}

/// The prepared payload `file` names in `folder`. Its header holds its own size.
pub(super) fn read(folder: &Path, file: &Value) -> anyhow::Result<Payload> {
    use anyhow::{Context, ensure};
    let name = file.as_str().context("Prepared payload file missing")?;
    let payload = Payload(fs::read(folder.join(name))?);
    ensure!(
        payload.u64(0)? == payload.0.len() as u64,
        "Prepared payload {name} has an invalid size"
    );
    Ok(payload)
}

/// `payload` as a private copy of the native clip `template`, sampled on `rig`.
pub(super) fn clip(
    manager: &sundial::package_authoring::PackageManager,
    template: u32,
    payload: Payload,
    rig: &animation::control::Rig,
) -> anyhow::Result<NewTagSpec> {
    anyhow::ensure!(
        manager
            .get_entry(TagHash(template))
            .is_some_and(|entry| entry.reference == CLIP_CLASS),
        "Prepared clip template 0x{template:08X} is not a native clip"
    );
    animation::clip::sample(&payload, &rig.defaults, 0)?;
    Ok(NewTagSpec {
        template_tag: TagHash(template),
        payload: payload.0,
        storage: crate::NewTagStorageMode::InheritTemplate,
    })
}

/// The bank descriptor each clip of `definitions` that names a source `descriptor` adds, in
/// order, with the tag each clip took, in the same order. A clip without one adds none of its own.
pub(super) fn additions(
    definitions: &[Value],
    tags: &[TagHash],
) -> anyhow::Result<Vec<animation::bank::Addition>> {
    use anyhow::Context;
    anyhow::ensure!(
        definitions.len() == tags.len(),
        "Prepared clips and their tags differ in number"
    );
    definitions
        .iter()
        .zip(tags)
        .filter(|(clip, _)| clip.get("descriptor").is_some())
        .map(|(clip, tag)| {
            Ok(animation::bank::Addition {
                descriptor: usize::try_from(word(clip, "descriptor")?)?,
                template: word(clip, "template")?,
                clip: tag.0,
                duration: clip["duration"]
                    .as_f64()
                    .context("Prepared clip duration missing")? as f32,
            })
        })
        .collect()
}

/// The asset group a bank's private clips take, and the tag each clip takes. These clips stay
/// resident in every activity, so a clip whose payload an earlier group of the same owner already
/// appended, or that repeats within this group, names that entry instead of taking another.
pub(super) struct Group {
    plan: Option<crate::asset_packages::GroupPlan>,
    /// The clips this group appends, with their digests and tags, in append order.
    added: Vec<([u8; 32], NewTagSpec, TagHash)>,
    /// The tag of each prepared clip, in the prepared order.
    pub(super) tags: Vec<TagHash>,
}

impl Group {
    pub(super) fn plan(emission: &PackageEmission, clips: Vec<NewTagSpec>) -> anyhow::Result<Self> {
        use anyhow::Context;
        use sha2::Digest;
        let assets = &emission.asset_packages;
        // Each prepared clip names an entry an earlier group appended, or one of this group's.
        let mut slots = Vec::with_capacity(clips.len());
        let mut fresh: Vec<([u8; 32], NewTagSpec)> = Vec::new();
        let mut within = std::collections::BTreeMap::new();
        for clip in clips {
            let digest: [u8; 32] = sha2::Sha256::digest(&clip.payload).into();
            if let Some(tag) = assets.shared_clips.get(&(assets.owner, digest)) {
                slots.push(Ok(*tag));
            } else if let Some(&index) = within.get(&digest) {
                slots.push(Err(index));
            } else {
                within.insert(digest, fresh.len());
                slots.push(Err(fresh.len()));
                fresh.push((digest, clip));
            }
        }
        let plan = (!fresh.is_empty())
            .then(|| assets.plan_group(fresh.iter().map(|(_, clip)| clip.payload.len())))
            .transpose()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let added = fresh
            .into_iter()
            .enumerate()
            .map(|(index, (digest, clip))| {
                let plan = plan.as_ref().context("Clip group plan missing")?;
                let tag = TagHash::new(plan.package_id, u16::try_from(plan.base + index)?);
                Ok((digest, clip, tag))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let tags = slots
            .into_iter()
            .map(|slot| slot.unwrap_or_else(|index| added[index].2))
            .collect();
        Ok(Self { plan, added, tags })
    }

    /// The runtime dependency companion, with every clip of the group enrolled in its root.
    pub(super) fn dependencies(
        &self,
        manager: &sundial::package_authoring::PackageManager,
        emission: &PackageEmission,
    ) -> anyhow::Result<Vec<u8>> {
        let dependencies = emission
            .runtime_dependencies
            .clone()
            .map(Ok)
            .unwrap_or_else(|| {
                manager
                    .read_tag(RUNTIME_DEPENDENCY_COMPANION)
                    .map_err(|error| anyhow::anyhow!("{error}"))
            })?;
        let tags = self
            .tags
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        crate::shared_tag_dependency_index::enroll_dependencies(
            &dependencies,
            RUNTIME_DEPENDENCY_COMPANION,
            RUNTIME_DEPENDENCY_ROOT,
            &tags.into_iter().collect::<Vec<_>>(),
        )
        .map_err(|error| anyhow::anyhow!("{error}"))
    }

    /// Appends the group's new clips, records them for later groups of the same owner and keeps
    /// `dependencies` as the build's companion.
    pub(super) fn commit(
        self,
        emission: &mut PackageEmission,
        dependencies: Vec<u8>,
    ) -> AuthoringResult<()> {
        let assets = &mut emission.asset_packages;
        if let Some(plan) = self.plan {
            let mut clips = Vec::with_capacity(self.added.len());
            let mut shared = Vec::with_capacity(self.added.len());
            for (digest, clip, tag) in self.added {
                clips.push(clip);
                shared.push(((assets.owner, digest), tag));
            }
            assets.commit_group(plan, clips, Vec::new())?;
            assets.shared_clips.extend(shared);
        }
        emission.runtime_dependencies = Some(dependencies);
        Ok(())
    }
}
