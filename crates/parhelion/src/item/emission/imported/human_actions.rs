//! Add an imported ability's or armor's private human actions to the shared native human tables. Each action
//! copies the same-named native node under a private name, and its clips join the bank after every
//! existing descriptor. Stock names, descriptors and clip rows keep their ordinals, and earlier
//! items' additions are kept. The stock records are replaced in the build's own packages. No stock
//! package file is written.
//!
//! Private action names identify the additions. Separate imports of one source prepare the same
//! names, so actions whose content matches actions already added are added once, and different
//! content under a name already added is refused.
//!
//! In a mod family whose main mod writes the shared character tables, an importer's build keeps
//! the clips and records the actions, and the main mod's build adds them to the tables.
use super::*;
use crate::mod_family::build::{HostedCharacter, Shared};
use parhelion_import::tiger::{animation, payload::Payload};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};

fn word(value: &Value, key: &str) -> anyhow::Result<u32> {
    use anyhow::Context;
    Ok(u32::try_from(
        value[key]
            .as_u64()
            .with_context(|| format!("Human action {key} missing"))?,
    )?)
}

/// Every imported ability directory the subclass specs name, and every imported armor graph,
/// whose evade can play actions of its own, each once.
pub(super) fn directories(weapons: &[WeaponCloneSpec]) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    for spec in weapons.iter().filter(|spec| spec.kind == ItemKind::Armor) {
        if let Some(graph) = &spec.overrides.imported_graph {
            found.insert(graph.directory.clone());
        }
    }
    for abilities in weapons
        .iter()
        .filter_map(|spec| spec.overrides.subclass_abilities.as_ref())
    {
        let edits = abilities.choices.iter().map(|choice| &choice.edits).chain(
            abilities
                .attunements
                .iter()
                .flat_map(|attunement| &attunement.nodes)
                .map(|node| &node.edits),
        );
        for edits in edits {
            if let Some(imported) = &edits.imported_ability {
                found.insert(imported.directory.clone());
            }
        }
    }
    found.into_iter().collect()
}

/// The content digest of a prepared section: the section without the names of its clip files,
/// then each clip payload. Two imports of one source match although their files differ in name.
fn content_digest(section: &Value, clips: &[NewTagSpec]) -> anyhow::Result<[u8; 32]> {
    let mut content = section.clone();
    for clip in content["clips"].as_array_mut().into_iter().flatten() {
        if let Some(clip) = clip.as_object_mut() {
            clip.remove("file");
        }
    }
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&content)?);
    for clip in clips {
        digest.update((clip.payload.len() as u64).to_le_bytes());
        digest.update(&clip.payload);
    }
    Ok(digest.finalize().into())
}

/// A graph's prepared human actions and the stock records they were prepared against.
struct Prepared {
    section: Value,
    privates: BTreeSet<u32>,
    originals: BTreeMap<&'static str, (u32, Payload)>,
}

/// The human actions the graph in `folder` prepared, or `None` when it adds none.
fn prepared(
    manager: &sundial::package_authoring::PackageManager,
    folder: &Path,
) -> anyhow::Result<Option<Prepared>> {
    use anyhow::{Context, ensure};
    let mut graph: Value = serde_json::from_slice(&fs::read(folder.join("asset-graph.json"))?)?;
    let section = graph["human_actions"].take();
    if section.is_null() {
        return Ok(None);
    }
    ensure!(
        section["schema"] == 1,
        "Reimport this ability's human actions"
    );
    let privates = section["actions"]
        .as_array()
        .context("Human actions missing")?
        .iter()
        .map(|action| word(action, "private"))
        .collect::<anyhow::Result<BTreeSet<_>>>()?;
    // Every stock record must be the one the actions were prepared against.
    let mut originals = BTreeMap::new();
    for key in [
        "lookup",
        "pose",
        "skeleton",
        "controls",
        "bank",
        "states",
        "layers",
        "directions",
    ] {
        let record = &section["stock"][key];
        let tag = word(record, "tag")?;
        let payload = Payload(manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)?);
        ensure!(
            hex::encode(Sha256::digest(&payload.0)) == record["sha256"],
            "Human {key} changed since this ability was prepared. Reimport it with the current packages"
        );
        originals.insert(key, (tag, payload));
    }
    let lookup = &originals["lookup"].1;
    let pose = &originals["pose"].1;
    let (lp, pp) = (lookup.pointer(24)?, pose.pointer(24)?);
    ensure!(
        lookup.u32(lp + 0x90)? == originals["bank"].0
            && lookup.u32(lp + 0x9C)? == originals["states"].0
            && pose.u32(pp + 0x108)? == originals["bank"].0
            && pose.u32(pp + 0x114)? == originals["directions"].0
            && pose.u32(pp + 0x11C)? == originals["layers"].0,
        "Human lookup and pose name different tables"
    );
    Ok(Some(Prepared {
        section,
        privates,
        originals,
    }))
}

/// Whether actions under these private names with this content are already in the tables.
/// Different content under a name already added is refused.
fn already_added(
    added: &BTreeMap<u32, [u8; 32]>,
    privates: &BTreeSet<u32>,
    digest: &[u8; 32],
) -> anyhow::Result<bool> {
    let earlier = privates
        .iter()
        .filter_map(|private| added.get(private))
        .collect::<Vec<_>>();
    if earlier.is_empty() {
        return Ok(false);
    }
    anyhow::ensure!(
        earlier.len() == privates.len() && earlier.iter().all(|&old| old == digest),
        "Another imported item adds different human actions under the same private names. Reimport one of them"
    );
    Ok(true)
}

/// The shared tables with the actions added on the `previous` payloads, each of the graph's
/// clips taking the tag at its place in `tags`.
fn extend(
    prepared: &Prepared,
    previous: &BTreeMap<u32, Vec<u8>>,
    tags: &[TagHash],
) -> anyhow::Result<Vec<ReplacementSpec>> {
    use anyhow::{Context, ensure};
    let Prepared {
        section, originals, ..
    } = prepared;
    let current = |key: &str| {
        let (tag, original) = &originals[key];
        Payload(
            previous
                .get(tag)
                .cloned()
                .unwrap_or_else(|| original.0.clone()),
        )
    };
    let definitions = section["clips"].as_array().context("Human clips missing")?;
    let additions = super::clips::additions(definitions, tags)?;
    ensure!(
        additions.len() == definitions.len(),
        "Every human clip needs its source descriptor"
    );
    let animation::bank::Extension {
        mut bank,
        layers,
        directions,
        descriptors,
    } = animation::bank::extend(
        &current("bank"),
        &current("layers"),
        &current("directions"),
        &originals["bank"].1,
        &originals["layers"].1,
        &originals["directions"].1,
        &additions,
    )?;
    // Each added descriptor takes its source descriptor's playback policy.
    let rows = bank.array(0x68, 32, Some(0x80809002))?;
    for (descriptor, clip) in descriptors.iter().zip(definitions) {
        let policy = hex::decode(
            clip["policy"]
                .as_str()
                .context("Human clip policy missing")?,
        )?;
        ensure!(policy.len() == 8, "Human clip policy differs");
        let at = rows[*descriptor] + 16;
        bank.0[at..at + 8].copy_from_slice(&policy);
    }
    let mut routes = animation::state::ChoiceRoutes::new();
    let mut names = BTreeMap::new();
    for action in section["actions"]
        .as_array()
        .context("Human actions missing")?
    {
        let name = word(action, "name")?;
        let choices = action["choices"]
            .as_array()
            .context("Human action choices missing")?
            .iter()
            .map(|choice| {
                let index = usize::try_from(choice[0].as_u64().context("Human choice clip")?)?;
                let weight = u32::try_from(choice[1].as_u64().context("Human choice weight")?)?;
                Ok((
                    *descriptors
                        .get(index)
                        .context("Human choice names an absent clip")?,
                    weight,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        ensure!(
            routes
                .insert(name, BTreeMap::from([(0, choices)]))
                .is_none()
                && names.insert(name, word(action, "private")?).is_none(),
            "Human actions overlap"
        );
    }
    let states = animation::state::extend_actions(&current("states"), &routes, &names)?;
    Ok([
        ("bank", bank),
        ("states", states),
        ("layers", layers),
        ("directions", directions),
    ]
    .into_iter()
    .map(|(key, payload)| ReplacementSpec {
        tag: TagHash(originals[key].0),
        payload: payload.0,
    })
    .collect())
}

/// `added` holds the content digest of the actions already added under each private name.
pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    folder: &Path,
    previous: &BTreeMap<u32, Vec<u8>>,
    added: &mut BTreeMap<u32, [u8; 32]>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    use anyhow::{Context, ensure};
    let prepare = || -> anyhow::Result<_> {
        let Some(prepared) = prepared(manager, folder)? else {
            return Ok(None);
        };
        let rig = animation::control::Rig::read_character(
            &prepared.originals["skeleton"].1,
            &prepared.originals["controls"].1,
        )?;
        let definitions = prepared.section["clips"]
            .as_array()
            .context("Human clips missing")?;
        ensure!(!definitions.is_empty(), "Human actions have no clips");
        let clips = definitions
            .iter()
            .map(|clip| {
                let payload = super::clips::read(folder, &clip["file"])?;
                super::clips::clip(manager, word(clip, "template")?, payload, &rig)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let digest = content_digest(&prepared.section, &clips)?;
        if already_added(added, &prepared.privates, &digest)? {
            return Ok(None);
        }
        let group = super::clips::Group::plan(emission, clips)?;
        let mut replacements = extend(&prepared, previous, &group.tags)?;
        super::character::compact(manager, &mut replacements)?;
        let dependencies = group.dependencies(manager, emission)?;
        Ok(Some((
            group,
            dependencies,
            replacements,
            prepared.privates,
            digest,
        )))
    };
    let Some((group, dependencies, replacements, privates, digest)) =
        prepare().map_err(|error| invalid(format!("Imported human actions: {error:#}")))?
    else {
        return Ok(Vec::new());
    };
    let allocated = group.tags.iter().map(|tag| tag.0).collect::<Vec<_>>();
    group.commit(emission, dependencies)?;
    for private in privates {
        added.insert(private, digest);
    }
    // The shared tables these actions change, and what the main mod's build needs to add them
    // there when it writes those tables for an importer. The clips stay in this build.
    crate::mod_family::build::produced(
        Shared::Character,
        replacements.iter().map(|replacement| replacement.tag.0),
    );
    crate::mod_family::build::contribute(|contributions| {
        contributions.human_actions.push(HostedCharacter {
            folder: folder.to_path_buf(),
            clips: allocated,
            digest,
        });
    });
    Ok(replacements)
}

/// Adds human actions an importer's build prepared to the shared tables the main mod's build
/// writes, with the clips the importer's build allocated in its own packages. Actions the main
/// mod's build already added are left as they are.
pub(super) fn host(
    manager: &sundial::package_authoring::PackageManager,
    hosted: &HostedCharacter,
    previous: &BTreeMap<u32, Vec<u8>>,
    added: &mut BTreeMap<u32, [u8; 32]>,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    let prepare = || -> anyhow::Result<_> {
        let prepared = prepared(manager, &hosted.folder)?.ok_or_else(|| {
            anyhow::anyhow!("The hosted human actions are missing from their graph")
        })?;
        if already_added(added, &prepared.privates, &hosted.digest)? {
            return Ok(None);
        }
        let tags = hosted
            .clips
            .iter()
            .copied()
            .map(TagHash)
            .collect::<Vec<_>>();
        let mut replacements = extend(&prepared, previous, &tags)?;
        super::character::compact(manager, &mut replacements)?;
        Ok(Some((prepared.privates, replacements)))
    };
    let Some((privates, replacements)) =
        prepare().map_err(|error| invalid(format!("Hosted human actions: {error:#}")))?
    else {
        return Ok(Vec::new());
    };
    for private in privates {
        added.insert(private, hosted.digest);
    }
    crate::mod_family::build::produced(
        Shared::Character,
        replacements.iter().map(|replacement| replacement.tag.0),
    );
    Ok(replacements)
}
