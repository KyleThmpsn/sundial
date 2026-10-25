//! A weapon's own copy of its arms rig, where single first-person actions play another weapon's
//! animations. The rules are in `weapon::animations::actions`.
//!
//! The copy runs from the action state table up: a private state table, the animation lookup
//! that names it, and the arms rig entity that holds the lookup. The weapon's attachment row then
//! names the private rig, so every other weapon on the stock rig keeps its own states.
use super::*;
use crate::recipe::AnimationAction;
use crate::weapon::animations::{Profile, actions::Machine, arms_rig, profile};

pub(super) fn author(
    manager: &PackageManager,
    entity: &[u8],
    selected: Option<u32>,
    actions: &[(AnimationAction, Profile)],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    if actions.is_empty() {
        return Ok(Vec::new());
    }
    let own = profile(manager, entity, selected)?;
    let rig = arms_rig(manager, entity, selected)?;
    let mut machine = Machine::read(rig.states.clone(), &rig.parameters)?;
    let mut changed = false;
    for (action, donor) in actions {
        if donor.owner != own.owner {
            return Err(invalid(format!(
                "{}: the chosen weapon's animations do not fit this model's rig. Choose another weapon.",
                action.label()
            )));
        }
        if donor.keys != own.keys {
            changed |= machine.take(*action, donor.keys[1])?;
        }
    }
    if !changed {
        return Ok(Vec::new());
    }
    let state_table = allocator
        .assigned_tag(tags.len(), "Animation action", "state table")?
        .0;
    tags.push(NewTagSpec {
        template_tag: TagHash(rig.states_tag),
        payload: machine.into_payload(),
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let mut lookup = rig.lookup.clone();
    let field = rig.states_field()?;
    lookup[field..field + 4].copy_from_slice(&state_table.to_le_bytes());
    let lookup_tag = allocator
        .assigned_tag(tags.len(), "Animation action", "animation lookup")?
        .0;
    retarget_weapon_component_owner_payload(&mut lookup, &rig.entity, rig.lookup_tag, lookup_tag)
        .map_err(invalid)?;
    tags.push(NewTagSpec {
        template_tag: TagHash(rig.lookup_tag),
        payload: lookup,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    let mut arms = rig.entity.clone();
    retarget_weapon_component_owner(&mut arms, rig.lookup_tag, lookup_tag).map_err(invalid)?;
    if arms
        .chunks_exact(4)
        .any(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]) == rig.lookup_tag)
    {
        return Err(invalid(
            "The private arms rig keeps a stale animation lookup reference",
        ));
    }
    let arms_tag = allocator
        .assigned_tag(tags.len(), "Animation action", "arms rig")?
        .0;
    tags.push(NewTagSpec {
        template_tag: TagHash(rig.entity_tag),
        payload: arms,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    Ok(vec![rig.patch(arms_tag)])
}
