//! Private copies of the entities a composed owner attaches, without chosen components.
//!
//! A sword guard attaches entity `80BBC2F6` to the player while it is active. Its third person
//! camera component (`80EF5842`, class `80808A0C`, as the sword entity's own always-on camera
//! `80EF588B` is) is what turns a glaive that guards into a third person weapon. The copy keeps
//! every other component and connection, and the composed owner names the copy instead, through
//! the same private graph copy that runtime resource patches with graph removals make.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttachmentEdit {
    /// The binding whose resource names the attached entity.
    binding_hash: u32,
    /// Offset of the entity reference from the start of that resource, as a runtime resource
    /// patch addresses it.
    offset: u32,
    /// The entity expected there, so an edit cannot follow a reference that moved.
    entity: u32,
    /// Component owners left out of the copy.
    remove_owners: BTreeSet<u32>,
}

pub(super) fn patches(
    manager: &PackageManager,
    edits: &[AttachmentEdit],
    entity: &[u8],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut patches = Vec::new();
    for edit in edits {
        if edit.remove_owners.is_empty() {
            return Err(invalid("An attachment edit removes no components"));
        }
        let bindings = weapon_component_bindings(entity, edit.binding_hash).map_err(invalid)?;
        let [binding] = bindings.as_slice() else {
            return Err(invalid(format!(
                "Attachment edit binding 0x{:08X} selects {} resources, not one",
                edit.binding_hash,
                bindings.len()
            )));
        };
        let owner = read_tag(manager, TagHash(binding.owner_tag), "attachment owner")?;
        let at = usize::try_from(binding.resource_offset)
            .ok()
            .and_then(|start| start.checked_add(edit.offset as usize))
            .ok_or_else(|| invalid("Attachment reference is outside its owner"))?;
        if read_u32(&owner, at)? != edit.entity {
            return Err(invalid(format!(
                "Attachment edit expects entity 0x{:08X} at 0x{:X} of owner 0x{:08X}",
                edit.entity, edit.offset, binding.owner_tag
            )));
        }
        let entry = manager
            .get_entry(TagHash(edit.entity))
            .ok_or_else(|| invalid("Attached entity is missing"))?;
        if entry.reference != WEAPON_ENTITY_CLASS {
            return Err(invalid("Attached reference is not a native entity"));
        }
        patches.push(WeaponRuntimeResourcePatch {
            binding_hash: edit.binding_hash,
            resource_index: 0,
            offset: edit.offset,
            bytes: edit.entity.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: edit.remove_owners.iter().copied().collect(),
            graph_trajectories: None,
        });
    }
    Ok(patches)
}
