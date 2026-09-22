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
use serde_json::Value;
use std::fs;

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
    /// Native clip tag and the converted payload replacing it, in bank order.
    pub clips: Vec<(u32, Vec<u8>)>,
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
    Ok(Some(ImportedAnimation {
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
        clips,
    }))
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

/// Append the private chain as weapon runtime tags and retarget the authored
/// entity. Returns the number of clips substituted.
pub(in crate::weapon) fn author(
    manager: &PackageManager,
    animation: &ImportedAnimation,
    pattern_entity_tag: TagHash,
    pattern_entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<usize> {
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
    let mut push = |template: u32, payload: Vec<u8>, what: &str| -> AuthoringResult<u32> {
        let assigned = allocator.assigned_tag(tags.len(), "Imported animation", what)?;
        tags.push(NewTagSpec {
            template_tag: TagHash(template),
            payload,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        Ok(assigned.0)
    };
    let mut bank = animation.bank.1.clone();
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
        let private = push(*native, payload.clone(), "clip")?;
        if relink(&mut bank, *native, private, "clip bank")? != 1 {
            return Err(invalid(format!(
                "Imported animation clip 0x{native:08X} appears more than once in the bank"
            )));
        }
    }
    let bank_tag = push(animation.bank.0, bank, "clip bank")?;
    let mut lookup = animation.lookup_owner.1.clone();
    relink(&mut lookup, animation.bank.0, bank_tag, "lookup owner")?;
    let lookup_tag = push(animation.lookup_owner.0, lookup, "lookup owner")?;
    let mut entity = animation.entity.1.clone();
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
    let entity_tag = push(animation.entity.0, entity, "first-person entity")?;
    let mut attachments = animation.attachment_owner.1.clone();
    relink(
        &mut attachments,
        animation.entity.0,
        entity_tag,
        "attachment owner",
    )?;
    let attachment_tag = push(
        animation.attachment_owner.0,
        attachments,
        "attachment owner",
    )?;
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
    Ok(animation.clips.len())
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
