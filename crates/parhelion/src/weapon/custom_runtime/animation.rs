//! Link an imported model's converted first-person clips into the authored
//! runtime weapon entity through private copies of the donor's animation chain.
//!
//! The prepared graph carries the native attachment owner, first-person entity,
//! lookup owner and clip bank payloads plus converted clips keyed by the native
//! clip they replace. Each link in that chain is a plain tag word, so every copy
//! is rewritten to name the next private tag and the authored runtime entity is
//! retargeted from the native attachment owner to the private one.
use super::*;
use parhelion_import::GraphReference;
use parhelion_import::d2_mot::rig_convert::animation::first_person::consumers;
use parhelion_import::d2_mot::rig_convert::animation::first_person::profile;
use serde_json::Value;
use std::{collections::BTreeMap, fs};

const FIRST_PERSON_ENTITY_CLASS: u32 = 0x8080_9C0F;
const RESOURCE_OWNER_CLASS: u32 = 0x8080_9C36;
const CLIP_BANK_CLASS: u32 = 0x8080_36F6;
const CLIP_CLASS: u32 = 0x8080_8F49;

/// Imported first-person clips and the native chain they replace.
pub(in crate::weapon) struct ImportedAnimation {
    pub runtime_entity: u32,
    pub attachment_owner: (u32, Vec<u8>),
    pub entity: (u32, Vec<u8>),
    pub lookup_owner: (u32, Vec<u8>),
    pub bank: (u32, Vec<u8>),
    pub converted_bank: Option<Vec<u8>>,
    pub extra_clips: Vec<(u32, u32, Vec<u8>)>,
    pub bank_consumers: Vec<(u32, Vec<u8>)>,
    /// Native clip tag and the converted payload replacing it, in bank order.
    pub clips: Vec<(u32, Vec<u8>)>,
    pub profile: Option<profile::Patch>,
    pub states: Option<ImportedStates>,
    pub rigs: Vec<ImportedRig>,
    pub pose_layers: Option<ImportedPoseLayers>,
}

pub(in crate::weapon) struct ImportedPoseLayers {
    owner: u32,
    original: (u32, Vec<u8>),
    converted: Vec<u8>,
}

pub(in crate::weapon) struct ImportedRig {
    original: (u32, Vec<u8>),
    converted: Vec<u8>,
    first_person: bool,
    kind: RigKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RigKind {
    Skeleton,
    Controls,
    Markers,
}

pub(in crate::weapon) struct ImportedStates {
    parameters: (u32, Vec<u8>),
    states: (u32, Vec<u8>),
    converted_parameters: Vec<u8>,
    converted_states: Vec<u8>,
}

impl ImportedAnimation {
    /// Upper bounds of the payloads `author` places in the weapon's asset group.
    pub(in crate::weapon) fn asset_bounds(&self) -> Vec<usize> {
        self.clips
            .iter()
            .map(|(_, payload)| payload.len())
            .chain(self.extra_clips.iter().map(|(_, _, payload)| payload.len()))
            .collect()
    }
}

fn tag(section: &Value, key: &str) -> AuthoringResult<u32> {
    u32::try_from(
        section[key]
            .as_u64()
            .ok_or_else(|| invalid(format!("Imported animation lacks {key}")))?,
    )
    .map_err(|_| invalid(format!("Imported animation {key} is not a tag")))
}

fn payload(graph: &GraphReference, file: &Value) -> AuthoringResult<Vec<u8>> {
    let name = file
        .as_str()
        .ok_or_else(|| invalid("Imported animation file name missing"))?;
    let path = graph.directory.join(name);
    if !path.starts_with(&graph.directory) || name.contains("..") {
        return Err(invalid("Imported animation file escapes its graph"));
    }
    fs::read(&path).map_err(|error| invalid(format!("Imported animation {name}: {error}")))
}

/// Read a graph's linked first-person animation, if it prepared one.
pub(in crate::weapon) fn load(
    graph: &GraphReference,
) -> AuthoringResult<Option<ImportedAnimation>> {
    let text = fs::read(graph.directory.join("asset-graph.json"))
        .map_err(|error| invalid(format!("Imported graph: {error}")))?;
    let value: Value = serde_json::from_slice(&text)
        .map_err(|error| invalid(format!("Imported graph: {error}")))?;
    let animation = &value["animation"];
    if animation["first_person_status"] != "linked" {
        return Ok(None);
    }
    let first_person = &animation["first_person"];
    let files = &first_person["files"];
    let mut clips = Vec::new();
    for clip in first_person["clips"]
        .as_array()
        .ok_or_else(|| invalid("Imported animation clips missing"))?
    {
        clips.push((tag(clip, "native")?, payload(graph, &clip["file"])?));
    }
    if clips.is_empty() {
        return Ok(None);
    }
    let states = if first_person["state_conversion"]["status"] == "source_states" {
        let section = &first_person["state_conversion"];
        Some(ImportedStates {
            parameters: (
                tag(section, "parameters")?,
                payload(graph, &files["parameters_template"])?,
            ),
            states: (
                tag(section, "states")?,
                payload(graph, &files["states_template"])?,
            ),
            converted_parameters: payload(graph, &files["parameters"])?,
            converted_states: payload(graph, &files["states"])?,
        })
    } else {
        None
    };
    let result = ImportedAnimation {
        runtime_entity: tag(animation, "runtime_entity")?,
        attachment_owner: (
            tag(first_person, "attachment_owner")?,
            payload(graph, &files["attachment_owner"])?,
        ),
        entity: (
            tag(first_person, "entity")?,
            payload(graph, &files["entity"])?,
        ),
        lookup_owner: (
            tag(first_person, "lookup_owner")?,
            payload(graph, &files["lookup_owner"])?,
        ),
        bank: (tag(first_person, "bank")?, payload(graph, &files["bank"])?),
        converted_bank: files
            .get("converted_bank")
            .map(|file| payload(graph, file))
            .transpose()?,
        extra_clips: first_person["extra_clips"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|clip| {
                Ok((
                    tag(clip, "source")?,
                    tag(clip, "template")?,
                    payload(graph, &clip["file"])?,
                ))
            })
            .collect::<AuthoringResult<_>>()?,
        bank_consumers: first_person["bank_consumers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|consumer| {
                let key = consumer["file_key"]
                    .as_str()
                    .ok_or_else(|| invalid("Imported animation consumer file key missing"))?;
                Ok((tag(consumer, "owner")?, payload(graph, &files[key])?))
            })
            .collect::<AuthoringResult<_>>()?,
        clips,
        states,
        pose_layers: first_person
            .get("pose_layers")
            .filter(|value| !value.is_null())
            .map(|layers| {
                Ok(ImportedPoseLayers {
                    owner: tag(layers, "owner")?,
                    original: (
                        tag(layers, "tag")?,
                        payload(graph, &layers["template_file"])?,
                    ),
                    converted: payload(graph, &layers["file"])?,
                })
            })
            .transpose()?,
        rigs: first_person["rigs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|rig| {
                Ok(ImportedRig {
                    kind: match rig.get("kind").and_then(Value::as_str) {
                        None | Some("skeleton") => RigKind::Skeleton,
                        Some("controls") => RigKind::Controls,
                        Some("markers") => RigKind::Markers,
                        Some(_) => return Err(invalid("Imported rig owner kind is unsupported")),
                    },
                    original: (tag(rig, "native")?, payload(graph, &rig["template_file"])?),
                    converted: payload(graph, &rig["file"])?,
                    first_person: rig["first_person"]
                        .as_bool()
                        .ok_or_else(|| invalid("Imported rig target missing"))?,
                })
            })
            .collect::<AuthoringResult<_>>()?,
        profile: first_person["attachment_profile"]
            .get("patch")
            .map(|patch| {
                serde_json::from_value(patch.clone())
                    .map_err(|error| invalid(format!("Imported animation profile: {error}")))
            })
            .transpose()?,
    };
    if let Some(bank) = &result.converted_bank {
        use parhelion_import::d2_mot::{
            payload::Payload, rig_convert::animation::first_person::dispatch,
        };
        if let Some(layers) = &result.pose_layers {
            dispatch::validate(&Payload(layers.converted.clone()), &Payload(bank.clone()))
                .map_err(|error| invalid(format!("Imported animation dispatch: {error:#}")))?;
        } else if read_u64(bank, 0x68)? != read_u64(&result.bank.1, 0x68)? {
            return Err(invalid(
                "Imported animation adds actions without a matching pose dispatch table. Refresh this import before building.",
            ));
        }
    }
    Ok(Some(result))
}

/// Rewrite every aligned word equal to `old` as `new`, requiring at least one.
fn relink(payload: &mut [u8], old: u32, new: u32, what: &str) -> AuthoringResult<usize> {
    let mut count = 0;
    for word in payload.chunks_exact_mut(4) {
        if u32::from_le_bytes([word[0], word[1], word[2], word[3]]) == old {
            word.copy_from_slice(&new.to_le_bytes());
            count += 1;
        }
    }
    if count == 0 {
        return Err(invalid(format!(
            "Imported animation {what} does not reference 0x{old:08X}"
        )));
    }
    Ok(count)
}

fn stock(
    manager: &PackageManager,
    (tag, payload): &(u32, Vec<u8>),
    class: u32,
    what: &str,
) -> AuthoringResult<()> {
    let entry = manager
        .get_entry(TagHash(*tag))
        .ok_or_else(|| invalid(format!("Imported animation {what} 0x{tag:08X} is not live")))?;
    if entry.reference != class {
        return Err(invalid(format!(
            "Imported animation {what} 0x{tag:08X} has class 0x{:08X}, expected 0x{class:08X}",
            entry.reference
        )));
    }
    if read_tag(manager, TagHash(*tag), what)? != *payload {
        return Err(invalid(format!(
            "Imported animation {what} 0x{tag:08X} changed since the graph was prepared"
        )));
    }
    Ok(())
}

/// Private animation payloads already appended, keyed by asset class and content.
///
/// Weapons of one source family share their clip banks, so the same converted
/// payload is requested by many weapons in a project. A package holds at most
/// 8192 entries, so identical payloads must become one tag rather than one per
/// weapon. Payloads are immutable data, so sharing a tag is safe.
pub(in crate::weapon) type AnimationTagCache = BTreeMap<(u32, Vec<u8>), u32>;

/// Source-owned rigs need both skeletons, one first-person controller at most per view and
/// the converted selectors that route to them. Each converted rig must still parse.
fn validate_rigs(manager: &PackageManager, animation: &ImportedAnimation) -> AuthoringResult<()> {
    if animation.rigs.is_empty() {
        return Ok(());
    }
    if animation
        .rigs
        .iter()
        .filter(|rig| rig.kind == RigKind::Skeleton)
        .count()
        != 2
        || animation
            .rigs
            .iter()
            .filter(|rig| rig.kind == RigKind::Skeleton && rig.first_person)
            .count()
            != 1
        || [false, true].into_iter().any(|first_person| {
            animation
                .rigs
                .iter()
                .filter(|rig| rig.kind == RigKind::Controls && rig.first_person == first_person)
                .count()
                > 1
        })
        || animation.states.is_none()
        || animation.profile.is_none()
    {
        return Err(invalid(
            "Source-owned rigs require both skeletons and converted animation selectors",
        ));
    }
    for rig in &animation.rigs {
        match rig.kind {
            RigKind::Controls => {
                parhelion_import::d2_mot::rig_convert::animation::first_person::controls::validate(
                    &rig.converted,
                )
                .map_err(|error| invalid(format!("Imported rig controls: {error:#}")))?;
            }
            RigKind::Skeleton => {
                parhelion_import::d2_mot::rig_convert::validate_native_instance(&rig.converted)
                    .map_err(|error| invalid(format!("Imported skeleton instance: {error:#}")))?;
            }
            RigKind::Markers => {
                if rig.first_person
                    || animation
                        .rigs
                        .iter()
                        .filter(|r| r.kind == RigKind::Markers)
                        .count()
                        != 1
                {
                    return Err(invalid("Imported runtime marker owner is ambiguous"));
                }
                parhelion_import::d2_mot::rig_convert::animation::first_person::markers::validate(
                    &rig.converted,
                )
                .map_err(|error| invalid(format!("Imported runtime markers: {error:#}")))?;
            }
        }
        stock(manager, &rig.original, RESOURCE_OWNER_CLASS, "rig owner")?;
    }
    Ok(())
}

/// Append the private chain and retarget the authored entity. The clips go to the weapon's
/// asset group and the bank, owners and entity that route them to the runtime tags. Returns
/// the number of clips substituted.
#[allow(clippy::too_many_arguments)]
pub(in crate::weapon) fn author(
    manager: &PackageManager,
    animation: &ImportedAnimation,
    pattern_entity_tag: TagHash,
    pattern_entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
    (assets, asset_tags): (AppendedTagAllocator, &mut Vec<NewTagSpec>),
    cache: &mut AnimationTagCache,
) -> AuthoringResult<(usize, u32)> {
    if pattern_entity_tag.0 != animation.runtime_entity {
        return Err(invalid(format!(
            "Imported animation was prepared against runtime entity 0x{:08X} but this weapon's pattern uses {pattern_entity_tag}",
            animation.runtime_entity
        )));
    }
    stock(
        manager,
        &animation.attachment_owner,
        RESOURCE_OWNER_CLASS,
        "attachment owner",
    )?;
    stock(
        manager,
        &animation.entity,
        FIRST_PERSON_ENTITY_CLASS,
        "first-person entity",
    )?;
    stock(
        manager,
        &animation.lookup_owner,
        RESOURCE_OWNER_CLASS,
        "lookup owner",
    )?;
    stock(manager, &animation.bank, CLIP_BANK_CLASS, "clip bank")?;
    for consumer in &animation.bank_consumers {
        stock(manager, consumer, RESOURCE_OWNER_CLASS, "bank consumer")?;
        let payload = parhelion_import::d2_mot::payload::Payload(consumer.1.clone());
        let at = consumers::bank_field(&payload)
            .map_err(|error| invalid(format!("Animation bank consumer: {error:#}")))?;
        if read_u32(&consumer.1, at)? != animation.bank.0 {
            return Err(invalid("Imported animation consumer bank changed"));
        }
    }
    if let Some(layers) = &animation.pose_layers {
        stock(manager, &layers.original, 0x808034EB, "pose layers")?;
        let consumers = animation
            .bank_consumers
            .iter()
            .filter(|(owner, _)| *owner == layers.owner)
            .collect::<Vec<_>>();
        if consumers.len() != 1
            || layers.converted.len() < layers.original.1.len()
            || read_u64(&layers.converted, 0)? != layers.converted.len() as u64
        {
            return Err(invalid("Imported pose layer controller or payload differs"));
        }
        let consumer = parhelion_import::d2_mot::payload::Payload(consumers[0].1.clone());
        let base = consumer
            .pointer(24)
            .map_err(|error| invalid(format!("Pose layer controller: {error:#}")))?;
        if read_u32(&consumer.0, base - 4)? != 0x80803640
            || read_u32(&consumer.0, base + 0x11C)? != layers.original.0
        {
            return Err(invalid("Imported pose layer binding changed"));
        }
    }
    if let Some(states) = &animation.states {
        stock(
            manager,
            &states.parameters,
            0x80808EE1,
            "animation parameter dictionary",
        )?;
        stock(manager, &states.states, 0x80803465, "animation state table")?;
    }
    validate_rigs(manager, animation)?;
    let mut push = |allocator: AppendedTagAllocator,
                    tags: &mut Vec<NewTagSpec>,
                    template: u32,
                    payload: Vec<u8>,
                    what: &str|
     -> AuthoringResult<u32> {
        // Key on the asset class rather than the template tag. Weapons of one
        // source family convert to the same payload while replacing different
        // native clips, so a template-keyed cache would never match.
        let class = manager
            .get_entry(TagHash(template))
            .ok_or_else(|| invalid(format!("Imported animation {what} template is not live")))?
            .reference;
        if let Some(existing) = cache.get(&(class, payload.clone())) {
            return Ok(*existing);
        }
        let assigned = allocator.assigned_tag(tags.len(), "Imported animation", what)?;
        cache.insert((class, payload.clone()), assigned.0);
        tags.push(NewTagSpec {
            template_tag: TagHash(template),
            payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        Ok(assigned.0)
    };
    let mut bank = animation
        .converted_bank
        .as_ref()
        .unwrap_or(&animation.bank.1)
        .clone();
    for (native, payload) in &animation.clips {
        let entry = manager.get_entry(TagHash(*native)).ok_or_else(|| {
            invalid(format!(
                "Imported animation clip 0x{native:08X} is not live"
            ))
        })?;
        if entry.reference != CLIP_CLASS {
            return Err(invalid(format!(
                "Imported animation clip 0x{native:08X} is not a native clip"
            )));
        }
        if payload.len() < 0x190 || read_u64(payload, 0)? != payload.len() as u64 {
            return Err(invalid(format!(
                "Imported animation clip for 0x{native:08X} has an invalid payload size"
            )));
        }
        let private = push(assets, asset_tags, *native, payload.clone(), "clip")?;
        // Several bank ordinals can name the same clip. Every alias must reach
        // the same private source clip while the controller keeps its ordinals.
        relink(&mut bank, *native, private, "clip bank")?;
    }
    for (source, template, payload) in &animation.extra_clips {
        if manager
            .get_entry(TagHash(*template))
            .is_none_or(|entry| entry.reference != CLIP_CLASS)
            || payload.len() < 0x190
            || read_u64(payload, 0)? != payload.len() as u64
        {
            return Err(invalid(
                "Imported source clip has an invalid template or payload",
            ));
        }
        let private = push(
            assets,
            asset_tags,
            *template,
            payload.clone(),
            "source clip",
        )?;
        relink(&mut bank, *source, private, "source clip bank")?;
    }
    let bank_tag = push(allocator, tags, animation.bank.0, bank, "clip bank")?;
    let mut lookup = animation.lookup_owner.1.clone();
    relink(&mut lookup, animation.bank.0, bank_tag, "lookup owner")?;
    if let Some(states) = &animation.states {
        let parameters = push(
            allocator,
            tags,
            states.parameters.0,
            states.converted_parameters.clone(),
            "animation parameter dictionary",
        )?;
        let state_table = push(
            allocator,
            tags,
            states.states.0,
            states.converted_states.clone(),
            "animation state table",
        )?;
        let at = parhelion_import::d2_mot::payload::Payload(lookup.clone())
            .pointer(24)
            .map_err(|error| invalid(format!("Animation lookup: {error:#}")))?;
        if read_u32(&lookup, at + 0x94)? != states.parameters.0
            || read_u32(&lookup, at + 0x9C)? != states.states.0
        {
            return Err(invalid("Animation lookup tables changed"));
        }
        lookup[at + 0x94..at + 0x98].copy_from_slice(&parameters.to_le_bytes());
        lookup[at + 0x9C..at + 0xA0].copy_from_slice(&state_table.to_le_bytes());
    }
    let lookup_tag = allocator
        .assigned_tag(tags.len(), "Imported animation", "lookup owner")?
        .0;
    retarget_weapon_component_owner_payload(
        &mut lookup,
        &animation.entity.1,
        animation.lookup_owner.0,
        lookup_tag,
    )
    .map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: TagHash(animation.lookup_owner.0),
        payload: lookup,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let mut entity = animation.entity.1.clone();
    for rig in &animation.rigs {
        let target: &mut [u8] = if rig.first_person {
            &mut entity
        } else {
            &mut *pattern_entity
        };
        let mut owner = rig.converted.clone();
        let assigned = allocator
            .assigned_tag(tags.len(), "Imported animation", "source rig owner")?
            .0;
        retarget_weapon_component_owner_payload(&mut owner, target, rig.original.0, assigned)
            .map_err(invalid)?;
        retarget_weapon_component_owner(target, rig.original.0, assigned).map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(rig.original.0),
            payload: owner,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    let pose_layer_tag = animation
        .pose_layers
        .as_ref()
        .map(|layers| {
            push(
                allocator,
                tags,
                layers.original.0,
                layers.converted.clone(),
                "pose layers",
            )
        })
        .transpose()?;
    for (original, bytes) in &animation.bank_consumers {
        let mut payload = parhelion_import::d2_mot::payload::Payload(bytes.clone());
        let at = consumers::bank_field(&payload)
            .map_err(|error| invalid(format!("Animation bank consumer: {error:#}")))?;
        payload.0[at..at + 4].copy_from_slice(&bank_tag.to_le_bytes());
        if let Some(layers) = &animation.pose_layers {
            if layers.owner == *original {
                let field = payload
                    .pointer(24)
                    .map_err(|error| invalid(format!("Pose layer controller: {error:#}")))?
                    + 0x11C;
                payload.0[field..field + 4].copy_from_slice(
                    &pose_layer_tag
                        .ok_or_else(|| invalid("Private pose layer tag missing"))?
                        .to_le_bytes(),
                );
            }
        }
        let assigned = allocator
            .assigned_tag(tags.len(), "Imported animation", "bank consumer")?
            .0;
        retarget_weapon_component_owner_payload(
            &mut payload.0,
            &animation.entity.1,
            *original,
            assigned,
        )
        .map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(*original),
            payload: payload.0,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        retarget_weapon_component_owner(&mut entity, *original, assigned).map_err(invalid)?;
    }
    retarget_weapon_component_owner(&mut entity, animation.lookup_owner.0, lookup_tag)
        .map_err(invalid)?;
    if entity
        .chunks_exact(4)
        .any(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == animation.lookup_owner.0)
    {
        return Err(invalid(
            "Imported animation first-person entity keeps a stale lookup owner reference",
        ));
    }
    let entity_tag = push(
        allocator,
        tags,
        animation.entity.0,
        entity,
        "first-person entity",
    )?;
    let mut attachments = animation.attachment_owner.1.clone();
    if let Some(patch) = &animation.profile {
        let mut payload = parhelion_import::d2_mot::payload::Payload(attachments);
        profile::apply(&mut payload, patch)
            .map_err(|error| invalid(format!("Imported animation profile: {error:#}")))?;
        attachments = payload.0;
    }
    relink(
        &mut attachments,
        animation.entity.0,
        entity_tag,
        "attachment owner",
    )?;
    // Audio authoring adds weapon-specific routes to this owner afterwards.
    // Sharing it through the immutable animation cache would let a later
    // weapon replace the sounds selected by an earlier weapon in the batch.
    let attachment_tag = allocator
        .assigned_tag(tags.len(), "Imported animation", "attachment owner")?
        .0;
    retarget_weapon_component_owner_payload(
        &mut attachments,
        pattern_entity,
        animation.attachment_owner.0,
        attachment_tag,
    )
    .map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: TagHash(animation.attachment_owner.0),
        payload: attachments,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    retarget_weapon_component_owner(pattern_entity, animation.attachment_owner.0, attachment_tag)
        .map_err(invalid)?;
    if pattern_entity
        .chunks_exact(4)
        .any(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == animation.attachment_owner.0)
    {
        return Err(invalid(
            "Authored runtime entity keeps a stale attachment owner reference",
        ));
    }
    Ok((animation.clips.len(), attachment_tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relink_rewrites_every_aligned_word_and_requires_one() {
        let mut bytes = vec![0u8; 20];
        bytes[0..4].copy_from_slice(&0x80C6_9D7Eu32.to_le_bytes());
        bytes[12..16].copy_from_slice(&0x80C6_9D7Eu32.to_le_bytes());
        // A misaligned coincidence is not a reference.
        bytes[5..9].copy_from_slice(&0x80C6_9D7Eu32.to_le_bytes());
        assert_eq!(
            relink(&mut bytes, 0x80C6_9D7E, 0x8253_0001, "bank").unwrap(),
            2
        );
        assert_eq!(read_u32(&bytes, 0).unwrap(), 0x8253_0001);
        assert_eq!(read_u32(&bytes, 12).unwrap(), 0x8253_0001);
        assert!(relink(&mut bytes, 0x80C6_9D7E, 0x8253_0002, "bank").is_err());
    }
}
