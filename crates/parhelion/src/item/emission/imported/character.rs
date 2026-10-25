//! Merge private character actions into shared native banks without moving stock ordinals.
use super::*;
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

pub(super) fn apply(
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    graph: &Value,
    folder: &Path,
    previous: &BTreeMap<u32, Vec<u8>>,
    completed: &mut BTreeMap<(u32, u32), [u8; 32]>,
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
        let read = |value: &Value| -> anyhow::Result<Payload> {
            let name = value.as_str().context("Character payload file missing")?;
            let payload = Payload(fs::read(folder.join(name))?);
            ensure!(
                payload.u64(0)? == payload.0.len() as u64,
                "Character payload has an invalid size"
            );
            Ok(payload)
        };
        let originals = snapshots(manager, character, &read, &mut digest)?;
        let tables = character["tables"]
            .as_array()
            .context("Character motion tables missing")?;
        let rig = animation::control::Rig::read_character(
            &originals["skeleton"].1,
            &originals["controls"].1,
        )?;
        let mut clips = Vec::new();
        for clip in character["clips"]
            .as_array()
            .context("Character clips missing")?
        {
            let template = word(clip, "template")?;
            // Earlier imports stored the animated span as floats. Re-encoding keeps each sample
            // within half a 16-bit step of its track's range.
            let payload = animation::clip::compact(&read(&clip["file"])?)?;
            ensure!(
                manager
                    .get_entry(TagHash(template))
                    .is_some_and(|entry| entry.reference == 0x80808F49),
                "Character clip template has an unsupported class"
            );
            animation::clip::sample(&payload, &rig.defaults, 0)?;
            digest.update(&payload.0);
            clips.push(NewTagSpec {
                template_tag: TagHash(template),
                payload: payload.0,
                storage: crate::NewTagStorageMode::InheritTemplate,
            });
        }
        ensure!(
            !clips.is_empty(),
            "Character animation has no private clips"
        );
        let digest: [u8; 32] = digest.finalize().into();
        if let Some(old) = completed.get(&(bank_tag, private)) {
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
        let dictionary = extend_parameter(
            &current("parameters"),
            builtin,
            word(parameter, "parent")?,
            private,
        )?;
        let plan = emission
            .asset_packages
            .plan_group(clips.iter().map(|clip| clip.payload.len()))
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let allocated = (0..clips.len())
            .map(|index| {
                Ok(TagHash::new(
                    plan.package_id,
                    u16::try_from(plan.base + index)?,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let additions = character["clips"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&allocated)
            .filter(|(clip, _)| clip.get("descriptor").is_some())
            .map(|(clip, tag)| {
                Ok(animation::bank::Addition {
                    descriptor: usize::try_from(word(clip, "descriptor")?)?,
                    template: word(clip, "template")?,
                    clip: tag.0,
                    duration: clip["duration"]
                        .as_f64()
                        .context("Character clip duration missing")?
                        as f32,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
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
        if parameter["alias"] == true {
            ensure!(
                builtin == 3 && character["actions"].as_array().is_some_and(Vec::is_empty),
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
                selector = animation::state::parameter::table_branch_from(
                    &selector,
                    &originals[&selector_key].1,
                    &dictionary,
                    builtin,
                    private,
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
        let dependencies = emission
            .runtime_dependencies
            .clone()
            .map(Ok)
            .unwrap_or_else(|| {
                manager
                    .read_tag(RUNTIME_DEPENDENCY_COMPANION)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })?;
        let dependencies = crate::shared_tag_dependency_index::enroll_dependencies(
            &dependencies,
            RUNTIME_DEPENDENCY_COMPANION,
            RUNTIME_DEPENDENCY_ROOT,
            &allocated,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
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
        Ok(Some((
            plan,
            clips,
            digest,
            private,
            bank_tag,
            dependencies,
            replacements,
            allocated,
            new_descriptors,
        )))
    };
    let Some((
        plan,
        clips,
        digest,
        private,
        bank,
        dependencies,
        replacements,
        allocated,
        descriptors,
    )) = prepare().map_err(|error| invalid(format!("Imported character actions: {error:#}")))?
    else {
        return Ok(Vec::new());
    };
    emission
        .asset_packages
        .commit_group(plan, clips, Vec::new())?;
    emission.runtime_dependencies = Some(dependencies);
    completed.insert((bank, private), digest);
    fs::write(folder.join("character-allocated.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "bank":bank,"private":private,"clips":allocated.iter().map(|tag| tag.0).collect::<Vec<_>>(),"descriptors":descriptors
    })).map_err(|e| invalid(e.to_string()))?).map_err(|e| invalid(e.to_string()))?;
    Ok(replacements)
}
