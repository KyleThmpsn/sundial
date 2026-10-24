//! First-person routing composes with prior private attachment edits on the same weapon.
use super::*;

pub(super) fn author(
    animation: &ImportedAnimation,
    first_person: u32,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<u32> {
    let bindings =
        sundial::package_authoring::entity::weapon_component_bindings(entity, 0xD3A5500E)
            .map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid("Imported animation requires one weapon attachment"));
    };
    let current = binding.owner_tag;
    let original = animation.attachment_owner.0;
    let mut existing = None;
    if current != original {
        for (index, tag) in tags.iter().enumerate() {
            if allocator
                .assigned_tag(index, "Imported animation", "attachment owner")?
                .0
                == current
            {
                if tag.template_tag.0 != original {
                    return Err(invalid("Private attachment uses another animation donor"));
                }
                existing = Some(index);
                break;
            }
        }
        if existing.is_none() {
            return Err(invalid(
                "Imported animation attachment is outside this weapon's private runtime",
            ));
        }
    }
    let mut payload = existing.map_or_else(
        || animation.attachment_owner.1.clone(),
        |index| tags[index].payload.clone(),
    );
    if let Some(patch) = &animation.profile {
        let mut patch = patch.clone();
        if let Some(binding) = animation.shared_profile {
            // Validate the prepared change against stock, then compose it with
            // the character binding already applied to this private owner.
            let mut original =
                parhelion_import::d2_mot::payload::Payload(animation.attachment_owner.1.clone());
            profile::apply(&mut original, &patch)
                .map_err(|error| invalid(format!("Prepared animation profile: {error:#}")))?;
            if existing.is_none()
                || patch.before != binding.parent
                || patch
                    .previous_category
                    .is_some_and(|category| category != patch.category)
            {
                return Err(invalid(
                    "First-person and character attachment profiles cannot be composed",
                ));
            }
            patch.before = binding.private;
            patch.after = binding.private;
        }
        let mut data = parhelion_import::d2_mot::payload::Payload(payload);
        profile::apply(&mut data, &patch)
            .map_err(|error| invalid(format!("Imported animation profile: {error:#}")))?;
        payload = data.0;
    }
    relink(
        &mut payload,
        animation.entity.0,
        first_person,
        "attachment owner",
    )?;
    // Character profiles and authored attachment settings may have already created
    // this private owner. Keep those bytes while replacing its first-person route.
    if let Some(index) = existing {
        tags[index].payload = payload;
        return Ok(current);
    }
    // Audio adds weapon-specific routes later, so this owner is never shared through
    // the immutable animation cache even when the first-person clips are identical.
    let assigned = allocator
        .assigned_tag(tags.len(), "Imported animation", "attachment owner")?
        .0;
    retarget_weapon_component_owner_payload(&mut payload, entity, original, assigned)
        .map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: TagHash(original),
        payload,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    retarget_weapon_component_owner(entity, original, assigned).map_err(invalid)?;
    if entity
        .chunks_exact(4)
        .any(|word| word == original.to_le_bytes())
    {
        return Err(invalid(
            "Authored runtime entity keeps a stale attachment owner reference",
        ));
    }
    Ok(assigned)
}
