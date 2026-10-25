//! Allocate damage profiles in the private runtime family before binding movement attack rows.
use super::*;
use crate::{NewTagSpec, NewTagStorageMode, appended_tags::AppendedTagAllocator};
use sundial::package_authoring::{ability_damage, sandbox_perk::program::AbilityRowModifier};

pub(super) fn resolve(
    manager: &PackageManager,
    row: &AbilityRow,
    bank: &[u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<Modifier> {
    let (AbilityRowModifier::Attack { amounts, .. }, Modifier::Attack(mut modifier)) =
        (&row.modifier, row.value())
    else {
        return Ok(row.value());
    };
    let profiles = modifier.profiles(bank).map_err(invalid)?;
    let mut private = [0; 2];
    for lane in 0..2 {
        let template = TagHash(profiles[lane]);
        if manager
            .get_entry(template)
            .is_none_or(|e| e.reference != 0x8080_9C36)
        {
            return Err(invalid(
                "Movement attack profile has another directory class",
            ));
        }
        let mut payload = manager
            .read_tag(template)
            .map_err(|e| invalid(e.to_string()))?;
        let profile = ability_damage::profile(template.0, &payload)
            .ok_or_else(|| invalid("Movement attack has another outgoing profile layout"))?;
        if crate::tag_payload::read_u64(&payload, profile.paired + 8)? != profile.root as u64
            || payload.get(profile.root + 0x53).copied() != Some(0)
        {
            return Err(invalid(
                "Movement attack profile has an invalid pair or clock scaling",
            ));
        }
        ability_damage::set_fixed_amount(&mut payload, profile, f32::from_bits(amounts[lane]))
            .map_err(invalid)?;
        private[lane] = allocator
            .assigned_tag(tags.len(), "Private ability", "attack damage profile")?
            .0;
        ability_damage::retype(&mut payload, profile, private[lane], profile.mode)
            .map_err(invalid)?;
        tags.push(NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        });
    }
    modifier.damage = Some(private);
    Ok(Modifier::Attack(modifier))
}
