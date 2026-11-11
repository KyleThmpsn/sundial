//! Merge private character actions into shared native banks without moving stock ordinals.
use super::*;
use crate::mod_family::build::{HostedCharacter, Shared};
use parhelion_import::tiger::{animation, payload::Payload};
use sha2::{Digest, Sha256};

fn word(value: &Value, key: &str) -> anyhow::Result<u32> {
    use anyhow::Context;
    Ok(u32::try_from(
        value[key]
            .as_u64()
            .with_context(|| format!("Character {key} missing"))?,
    )?)
}

fn snapshots(
    manager: &sundial::package_authoring::PackageManager,
    character: &Value,
    read: &impl Fn(&Value) -> anyhow::Result<Payload>,
    digest: &mut Sha256,
) -> anyhow::Result<BTreeMap<String, (u32, Payload)>> {
    use anyhow::{Context, ensure};
    let mut originals = BTreeMap::new();
    for (key, class) in [
        ("bank", 0x808036F6),
        ("parameters", 0x80808EE1),
        ("states", 0x80803465),
        ("layers", 0x808034EB),
        ("pose", 0x80809C36),
        ("directions", 0x8080348E),
        ("skeleton", 0x80809C36),
        ("controls", 0x80809C36),
    ] {
        let tag = word(character, key)?;
        let payload = read(&character["files"][key])?;
        ensure!(
            manager
                .get_entry(TagHash(tag))
                .is_some_and(|entry| entry.reference == class)
                && manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)? == payload.0,
            "Character {key} was prepared from different or modified native packages. Reimport this item with the current importer before building"
        );
        digest.update(&payload.0);
        originals.insert(key.to_owned(), (tag, payload));
    }
    if character.get("first_person_parameters").is_some() {
        let key = "first_person_parameters";
        let tag = word(character, key)?;
        let payload = read(&character["files"][key])?;
        ensure!(
            tag != originals["parameters"].0
                && manager
                    .get_entry(TagHash(tag))
                    .is_some_and(|e| e.reference == 0x80808EE1)
                && manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)? == payload.0,
            "First-person parameter dictionary changed since preparation"
        );
        digest.update(&payload.0);
        originals.insert(key.to_owned(), (tag, payload));
    }
    let pose = &originals["pose"].1;
    let pose_at = pose.pointer(24)?;
    ensure!(
        pose.u32(pose_at - 4)? == 0x80803640
            && pose.u32(pose_at + 0x108)? == word(character, "bank")?
            && pose.u32(pose_at + 0x114)? == originals["directions"].0
            && pose.u32(pose_at + 0x11C)? == originals["layers"].0,
        "Character pose references a different bank or correction table"
    );
    let tables = character["tables"]
        .as_array()
        .context("Character motion tables missing")?;
    for table in tables {
        let label = table["kind"]
            .as_str()
            .context("Character table kind missing")?;
        let class = match label {
            "rider" => 0x8080344D,
            "stance" => 0x8080345F,
            _ => anyhow::bail!("Unsupported character motion table"),
        };
        ensure!(
            word(table, "class")? == class,
            "Character motion table class differs"
        );
        for (suffix, class) in [("table", class), ("selector", 0x80803478)] {
            let key = format!("{label}_{suffix}");
            let tag = word(table, suffix)?;
            let payload = read(&character["files"][&key])?;
            ensure!(
                manager
                    .get_entry(TagHash(tag))
                    .is_some_and(|entry| entry.reference == class)
                    && manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)? == payload.0,
                "Character {key} was prepared from different or modified native packages. Reimport this item with the current importer before building"
            );
            digest.update(&payload.0);
            ensure!(
                originals.insert(key, (tag, payload)).is_none(),
                "Character motion tables overlap"
            );
        }
    }
    Ok(originals)
}

/// The private character profiles the build has added: each name with the digest of what it
/// added, and the first profile of each shared content with the descriptors its clips took.
#[derive(Clone, Default)]
pub(super) struct Profiles {
    completed: BTreeMap<(u32, u32), [u8; 32]>,
    contents: BTreeMap<[u8; 32], (u32, Vec<usize>)>,
}

impl Profiles {
    /// Records the profile `name` as added, and as the one its `content` names when it is the
    /// first of that content.
    fn remember(
        &mut self,
        name: (u32, u32),
        digest: [u8; 32],
        content: Option<[u8; 32]>,
        descriptors: &[usize],
    ) {
        self.completed.insert(name, digest);
        if let Some(content) = content {
            self.contents
                .entry(content)
                .or_insert_with(|| (name.1, descriptors.to_vec()));
        }
    }
}

/// What a lookup-alias stance profile adds to the shared tables apart from its own name: its
/// stock tables, binding parent, clips by the entries they took, and its motion routes. Importers
/// name each item's profile after the item, while many items convert the same clips, so a profile
/// whose content matches an earlier one selects that profile's branch instead of forking the
/// selector again. Each fork copies the whole stock tree, and the selector's signed 16-bit indices
/// held about 125 forks. A profile with actions, rider roles or branch conditions names itself in
/// more places than one gate, so it keeps a fork of its own (`None`).
fn content_key(character: &Value, allocated: &[TagHash]) -> anyhow::Result<Option<[u8; 32]>> {
    use anyhow::{Context, ensure};
    let tables = character["tables"]
        .as_array()
        .context("Character motion tables missing")?;
    let branches = || {
        tables
            .iter()
            .flat_map(|table| table["branches"].as_array().into_iter().flatten())
    };
    if character["binding"]["alias"] != true
        || character["actions"]
            .as_array()
            .is_none_or(|actions| !actions.is_empty())
        || character["roles"]
            .as_array()
            .is_some_and(|roles| !roles.is_empty())
        || branches().any(|branch| {
            branch.get("private").is_some()
                || branch["conditions"]
                    .as_object()
                    .is_none_or(|conditions| !conditions.is_empty())
        })
    {
        return Ok(None);
    }
    let clips = character["clips"]
        .as_array()
        .context("Character clips missing")?;
    ensure!(
        clips.len() == allocated.len(),
        "Prepared clips and their tags differ in number"
    );
    let stock = [
        "bank",
        "parameters",
        "states",
        "layers",
        "pose",
        "directions",
        "skeleton",
        "controls",
        "first_person_parameters",
    ]
    .map(|key| character.get(key).cloned());
    let content = serde_json::json!({
        "tables": stock,
        "builtin": character["binding"]["builtin"],
        "parent": character["binding"]["parent"],
        "clips": clips.iter().zip(allocated).map(|(clip, tag)| serde_json::json!({
            "tag": tag.0,
            "template": clip["template"],
            "descriptor": clip.get("descriptor"),
            "duration": clip["duration"],
            "holding_layers": clip.get("holding_layers"),
        })).collect::<Vec<_>>(),
        "motion": tables.iter().map(|table| serde_json::json!({
            "kind": table["kind"],
            "class": table["class"],
            "table": table["table"],
            "selector": table["selector"],
            "branches": table["branches"],
        })).collect::<Vec<_>>(),
    });
    Ok(Some(Sha256::digest(serde_json::to_vec(&content)?).into()))
}

/// The shared tables with `character`'s private name selecting the branch the earlier profile
/// `existing` added for the same content: its lookup aliases and a gate record in each motion
/// table selector. The bank, layers, directions, states and motion tables stay as they are.
fn share_tables(
    manager: &sundial::package_authoring::PackageManager,
    character: &Value,
    originals: &BTreeMap<String, (u32, Payload)>,
    current: &dyn Fn(&str) -> Payload,
    existing: u32,
) -> anyhow::Result<Vec<ReplacementSpec>> {
    use anyhow::{Context, ensure};
    let private = word(character, "private")?;
    let parameter = &character["binding"];
    let builtin = u8::try_from(word(parameter, "builtin")?)?;
    ensure!(
        matches!(builtin, 2 | 3) && word(parameter, "private")? == private,
        "Character binding does not publish its private parameter"
    );
    let parent = word(parameter, "parent")?;
    let dictionary =
        animation::state::parameter::alias(&current("parameters"), builtin, parent, private)?;
    let mut replacements = Vec::new();
    for table in character["tables"]
        .as_array()
        .context("Character motion tables missing")?
    {
        let key = format!(
            "{}_selector",
            table["kind"]
                .as_str()
                .context("Character table kind missing")?
        );
        let selector = animation::state::parameter::table_share(
            &current(&key),
            &dictionary,
            builtin,
            existing,
            private,
        )?;
        replacements.push(ReplacementSpec {
            tag: TagHash(originals[&key].0),
            payload: selector.0,
        });
    }
    if let Some((tag, _)) = originals.get("first_person_parameters") {
        let payload = animation::state::parameter::alias(
            &current("first_person_parameters"),
            3,
            parent,
            private,
        )?;
        replacements.push(ReplacementSpec {
            tag: TagHash(*tag),
            payload: payload.0,
        });
    }
    replacements.push(ReplacementSpec {
        tag: TagHash(originals["parameters"].0),
        payload: dictionary.0,
    });
    compact(manager, &mut replacements)?;
    Ok(replacements)
}

/// Keeps only what each root of `replacements` reaches. Each extension appends whole arrays and
/// leaves the arrays it replaced behind, and the next imported character extends these same
/// payloads, so the shared tables grow with what they hold rather than with every earlier copy.
/// A payload the compactor cannot walk stays as extended. Human actions extend the same bank,
/// states, layers and directions, so they compact them too.
pub(super) fn compact(
    manager: &sundial::package_authoring::PackageManager,
    replacements: &mut [ReplacementSpec],
) -> anyhow::Result<()> {
    use anyhow::Context;
    for replacement in replacements {
        let class = manager
            .get_entry(replacement.tag)
            .map(|entry| entry.reference)
            .context("Character table entry missing")?;
        if let Ok(compacted) =
            parhelion_import::tiger::payload::compact(&Payload(replacement.payload.clone()), class)
        {
            replacement.payload = compacted.payload.0;
        }
    }
    Ok(())
}

/// The stock swing cue each prepared swing record names until it is bound to its private cue.
const SWING_TEMPLATE: u32 = 0x80B4F754;

/// Bind the prepared swing records of one clip to the private cues linked with the graph.
fn bind_swings(
    payload: &mut Payload,
    clip: &Value,
    symbols: &BTreeMap<String, TagHash>,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    for cue in clip["cues"].as_array().into_iter().flatten() {
        let offset = usize::try_from(cue["offset"].as_u64().context("Swing record offset")?)?;
        let symbol = cue["symbol"].as_str().context("Swing cue symbol")?;
        let tag = symbols
            .get(symbol)
            .with_context(|| format!("Swing cue {symbol} was not linked"))?;
        ensure!(
            offset >= 28
                && payload.u32(offset - 28)? == 0x80809033
                && payload.u32(offset)? == SWING_TEMPLATE,
            "Swing record does not match its prepared clip"
        );
        payload.0[offset..offset + 4].copy_from_slice(&tag.0.to_le_bytes());
    }
    Ok(())
}

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    graph: &Value,
    folder: &Path,
    symbols: &BTreeMap<String, TagHash>,
    previous: &BTreeMap<u32, Vec<u8>>,
    profiles: &mut Profiles,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    use anyhow::{Context, ensure};
    let Some(character) = graph["animation"].get("character") else {
        return Ok(Vec::new());
    };
    let prepare = || -> anyhow::Result<_> {
        ensure!(
            character["schema"] == 2,
            "Reimport this item's character animations. Older prepared assets omit direction tables required by added clips"
        );
        let private = word(character, "private")?;
        let bank_tag = word(character, "bank")?;
        let mut digest = Sha256::new();
        digest.update(serde_json::to_vec(character)?);
        let read = |value: &Value| super::clips::read(folder, value);
        let originals = snapshots(manager, character, &read, &mut digest)?;
        let rig = animation::control::Rig::read_character(
            &originals["skeleton"].1,
            &originals["controls"].1,
        )?;
        let mut clips = Vec::new();
        for definition in character["clips"]
            .as_array()
            .context("Character clips missing")?
        {
            let mut prepared = read(&definition["file"])?;
            bind_swings(&mut prepared, definition, symbols)?;
            // Earlier imports stored the animated span as floats. Re-encoding keeps each sample
            // within half a 16-bit step of its track's range.
            let payload = animation::clip::compact(&prepared)?;
            let clip = super::clips::clip(manager, word(definition, "template")?, payload, &rig)?;
            digest.update(&clip.payload);
            clips.push(clip);
        }
        ensure!(
            !clips.is_empty(),
            "Character animation has no private clips"
        );
        let digest: [u8; 32] = digest.finalize().into();
        if let Some(old) = profiles.completed.get(&(bank_tag, private)) {
            ensure!(
                *old == digest,
                "Conflicting private character profiles share one name"
            );
            return Ok(None);
        }
        let current = |key: &str| {
            let (tag, original) = &originals[key];
            Payload(
                previous
                    .get(tag)
                    .cloned()
                    .unwrap_or_else(|| original.0.clone()),
            )
        };
        let group = super::clips::Group::plan(emission, clips)?;
        let content = content_key(character, &group.tags)?;
        let (replacements, new_descriptors) =
            match content.and_then(|content| profiles.contents.get(&content)) {
                Some((existing, descriptors)) => (
                    share_tables(manager, character, &originals, &current, *existing)?,
                    descriptors.clone(),
                ),
                None => extend_tables(manager, character, &originals, &current, &group.tags)?,
            };
        // Swing cues stay in the item's own loading group. A scoped asset may not also join the
        // global root, and the private clips only play while this item is held.
        let dependencies = group.dependencies(manager, emission)?;
        Ok(Some((
            group,
            (digest, content),
            private,
            bank_tag,
            dependencies,
            replacements,
            new_descriptors,
        )))
    };
    let Some((group, (digest, content), private, bank, dependencies, replacements, descriptors)) =
        prepare().map_err(|error| invalid(format!("Imported character actions: {error:#}")))?
    else {
        return Ok(Vec::new());
    };
    let allocated = group.tags.clone();
    group.commit(emission, dependencies)?;
    profiles.remember((bank, private), digest, content, &descriptors);
    fs::write(folder.join("character-allocated.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "bank":bank,"private":private,"clips":allocated.iter().map(|tag| tag.0).collect::<Vec<_>>(),"descriptors":descriptors
    })).map_err(|e| invalid(e.to_string()))?).map_err(|e| invalid(e.to_string()))?;
    // The shared tables this profile changes, and what the main mod's build needs to add it
    // there when it writes them for an importer.
    crate::mod_family::build::produced(
        Shared::Character,
        replacements.iter().map(|replacement| replacement.tag.0),
    );
    crate::mod_family::build::contribute(|contributions| {
        contributions.characters.push(HostedCharacter {
            folder: folder.to_path_buf(),
            clips: allocated.iter().map(|tag| tag.0).collect(),
            digest,
        });
    });
    Ok(replacements)
}

/// The shared character tables with one private profile's actions added: the dictionaries, the
/// bank, states, layers, directions and motion tables, from the `current` payloads, the
/// `originals` the graph was prepared from and the tag each of the graph's clips took, in the
/// graph's clip order. Returns the replacements and the descriptor each body clip took.
fn extend_tables(
    manager: &sundial::package_authoring::PackageManager,
    character: &Value,
    originals: &BTreeMap<String, (u32, Payload)>,
    current: &dyn Fn(&str) -> Payload,
    allocated: &[TagHash],
) -> anyhow::Result<(Vec<ReplacementSpec>, Vec<usize>)> {
    use anyhow::{Context, ensure};
    let private = word(character, "private")?;
    let tables = character["tables"]
        .as_array()
        .context("Character motion tables missing")?;
    let parameter = &character["binding"];
    let builtin = u8::try_from(word(parameter, "builtin")?)?;
    ensure!(
        matches!(builtin, 2 | 3) && word(parameter, "private")? == private,
        "Character binding does not publish its private parameter"
    );
    let extend_parameter = if parameter["alias"] == true {
        animation::state::parameter::alias
    } else {
        animation::state::parameter::extend
    };
    let mut dictionary = extend_parameter(
        &current("parameters"),
        builtin,
        word(parameter, "parent")?,
        private,
    )?;
    // A vehicle's seated children each take a private rider role of their own, which descends
    // from their stock seat's role as the driver's descends from the carrier seat's.
    for role in character["roles"].as_array().into_iter().flatten() {
        ensure!(builtin == 2, "Only rider profiles carry additional roles");
        dictionary = animation::state::parameter::extend(
            &dictionary,
            builtin,
            word(role, "parent")?,
            word(role, "private")?,
        )?;
    }
    let additions = super::clips::additions(
        character["clips"]
            .as_array()
            .context("Character clips missing")?,
        allocated,
    )?;
    let animation::bank::Extension {
        mut bank,
        mut layers,
        mut directions,
        descriptors: new_descriptors,
    } = animation::bank::extend(
        &current("bank"),
        &current("layers"),
        &current("directions"),
        &originals["bank"].1,
        &originals["layers"].1,
        &originals["directions"].1,
        &additions,
    )?;
    let body_count = additions.len();
    let definitions = character["clips"].as_array().unwrap();
    ensure!(
        definitions[..body_count]
            .iter()
            .all(|c| c.get("descriptor").is_some())
            && definitions[body_count..]
                .iter()
                .all(|c| c.get("descriptor").is_none()),
        "Supplemental clips must follow body descriptors"
    );
    // Holding layers come only with an aliased stance profile. An alias without them is a
    // weapon profile whose translated actions and stances need no supplemental clips.
    let holding = definitions[..body_count]
        .iter()
        .any(|clip| clip.get("holding_layers").is_some());
    if holding {
        ensure!(
            parameter["alias"] == true
                && builtin == 3
                && character["actions"].as_array().is_some_and(Vec::is_empty),
            "Holding aliases require a weapon stance profile"
        );
        let holds = definitions[..body_count]
            .iter()
            .zip(&new_descriptors)
            .map(|(clip, &descriptor)| {
                let layers = clip["holding_layers"]
                    .as_array()
                    .context("Source holding layers")?
                    .iter()
                    .map(|layer| {
                        Ok(animation::bank::holding::Layer {
                            name: word(layer, "name")?,
                            clips: layer["clips"]
                                .as_array()
                                .context("Source holding weights")?
                                .iter()
                                .map(|weight| {
                                    Ok((
                                        usize::try_from(word(weight, "clip")?)?,
                                        weight["weight"]
                                            .as_f64()
                                            .context("Source holding weight")?
                                            as f32,
                                    ))
                                })
                                .collect::<anyhow::Result<_>>()?,
                        })
                    })
                    .collect::<anyhow::Result<_>>()?;
                Ok(animation::bank::holding::Hold {
                    original: usize::try_from(word(clip, "descriptor")?)?,
                    descriptor,
                    layers,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        animation::bank::holding::extend(
            &mut bank,
            &mut layers,
            &mut directions,
            &originals["layers"].1,
            &originals["directions"].1,
            &allocated[body_count..]
                .iter()
                .map(|t| t.0)
                .collect::<Vec<_>>(),
            &allocated.iter().map(|t| t.0).collect::<Vec<_>>(),
            &holds,
        )?;
    } else {
        ensure!(
            body_count == definitions.len(),
            "Supplemental clips require a holding profile"
        );
    }
    let remap = |value: &Value| -> anyhow::Result<BTreeMap<usize, usize>> {
        let mut mapping = BTreeMap::new();
        for (old, new) in value.as_object().context("Character route clips missing")? {
            let old: usize = old.parse()?;
            let index = usize::try_from(new.as_u64().context("Character clip index missing")?)?;
            ensure!(
                word(&character["clips"][index], "descriptor")? as usize == old,
                "Character route selects the wrong clip descriptor"
            );
            mapping.insert(
                old,
                *new_descriptors
                    .get(index)
                    .context("Character clip index outside its bank")?,
            );
        }
        Ok(mapping)
    };
    let mut routes = BTreeMap::new();
    for action in character["actions"]
        .as_array()
        .context("Character actions missing")?
    {
        let mapping = remap(&action["clips"])?;
        ensure!(
            routes.insert(word(action, "name")?, mapping).is_none(),
            "Character actions overlap"
        );
    }
    let states = if routes.is_empty() {
        current("states")
    } else {
        animation::state::parameter::branch(
            &current("states"),
            &originals["states"].1,
            &dictionary,
            builtin,
            private,
            &routes,
        )?
    };
    let mut extra = Vec::new();
    if let Some((tag, _)) = originals.get("first_person_parameters") {
        let payload = animation::state::parameter::alias(
            &current("first_person_parameters"),
            3,
            word(parameter, "parent")?,
            private,
        )?;
        extra.push(ReplacementSpec {
            tag: TagHash(*tag),
            payload: payload.0,
        });
    }
    for table in tables {
        let label = table["kind"].as_str().unwrap();
        let kind = match label {
            "rider" => animation::motion_table::Kind::Rider,
            "stance" => animation::motion_table::Kind::Stance,
            _ => unreachable!(),
        };
        let table_key = format!("{label}_table");
        let selector_key = format!("{label}_selector");
        let mut data = current(&table_key);
        let mut selector = current(&selector_key);
        let branches = table["branches"]
            .as_array()
            .context("Character table branches missing")?;
        ensure!(!branches.is_empty(), "Character table has no branches");
        for branch in branches {
            let mut routes = BTreeMap::new();
            for (choice, fields) in branch["routes"]
                .as_object()
                .context("Character table routes missing")?
            {
                let mut mapped = BTreeMap::new();
                for (field, clips) in fields
                    .as_object()
                    .context("Character table fields missing")?
                {
                    mapped.insert(field.parse()?, remap(clips)?);
                }
                routes.insert(choice.parse()?, mapped);
            }
            let conditions = branch["conditions"]
                .as_object()
                .context("Character table conditions missing")?
                .keys()
                .map(|key| Ok((key.parse()?, word(&branch["conditions"], key)?)))
                .collect::<anyhow::Result<BTreeMap<u8, u32>>>()?;
            let (extended, choices) = animation::motion_table::extend(&data, kind, &routes)?;
            // A seated child's branch names its own role, the driver's the profile's.
            let branch_private = match branch.get("private") {
                Some(_) => word(branch, "private")?,
                None => private,
            };
            selector = animation::state::parameter::table_branch_from(
                &selector,
                &originals[&selector_key].1,
                &dictionary,
                builtin,
                branch_private,
                &choices,
                &conditions,
            )?;
            data = extended;
        }
        extra.push(ReplacementSpec {
            tag: TagHash(originals[&table_key].0),
            payload: data.0,
        });
        extra.push(ReplacementSpec {
            tag: TagHash(originals[&selector_key].0),
            payload: selector.0,
        });
    }
    let mut replacements = [
        ("bank", bank),
        ("parameters", dictionary),
        ("states", states),
        ("layers", layers),
        ("directions", directions),
    ]
    .into_iter()
    .map(|(key, payload)| ReplacementSpec {
        tag: TagHash(originals[key].0),
        payload: payload.0,
    })
    .collect::<Vec<_>>();
    replacements.extend(extra);
    compact(manager, &mut replacements)?;
    Ok((replacements, new_descriptors))
}

/// Adds a character profile an importer's build prepared to the shared tables the main mod's
/// build writes, with the clips the importer's build allocated in its own packages. A profile
/// the main mod's build already added is left as it is.
pub(super) fn host(
    manager: &sundial::package_authoring::PackageManager,
    hosted: &HostedCharacter,
    previous: &BTreeMap<u32, Vec<u8>>,
    profiles: &mut Profiles,
) -> AuthoringResult<Vec<ReplacementSpec>> {
    use anyhow::{Context, ensure};
    let folder = hosted.folder.as_path();
    let prepare = || -> anyhow::Result<_> {
        let graph: Value = serde_json::from_slice(&fs::read(folder.join("asset-graph.json"))?)?;
        let character = graph["animation"]
            .get("character")
            .context("The hosted character profile is missing from its graph")?;
        let private = word(character, "private")?;
        let bank = word(character, "bank")?;
        if let Some(old) = profiles.completed.get(&(bank, private)) {
            ensure!(
                *old == hosted.digest,
                "Conflicting private character profiles share one name"
            );
            return Ok(None);
        }
        let read = |value: &Value| super::clips::read(folder, value);
        let originals = snapshots(manager, character, &read, &mut Sha256::new())?;
        let current = |key: &str| {
            let (tag, original) = &originals[key];
            Payload(
                previous
                    .get(tag)
                    .cloned()
                    .unwrap_or_else(|| original.0.clone()),
            )
        };
        let clips = hosted
            .clips
            .iter()
            .copied()
            .map(TagHash)
            .collect::<Vec<_>>();
        // A hosted profile shares an earlier one's branch on the same terms as the importer's own
        // build did, since its clips keep the entries that build gave them.
        let content = content_key(character, &clips)?;
        let (replacements, descriptors) =
            match content.and_then(|content| profiles.contents.get(&content)) {
                Some((existing, descriptors)) => (
                    share_tables(manager, character, &originals, &current, *existing)?,
                    descriptors.clone(),
                ),
                None => extend_tables(manager, character, &originals, &current, &clips)?,
            };
        Ok(Some((bank, private, content, replacements, descriptors)))
    };
    let Some((bank, private, content, replacements, descriptors)) =
        prepare().map_err(|error| invalid(format!("Hosted character actions: {error:#}")))?
    else {
        return Ok(Vec::new());
    };
    profiles.remember((bank, private), hosted.digest, content, &descriptors);
    crate::mod_family::build::produced(
        Shared::Character,
        replacements.iter().map(|replacement| replacement.tag.0),
    );
    Ok(replacements)
}
