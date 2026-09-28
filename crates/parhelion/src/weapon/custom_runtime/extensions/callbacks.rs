//! Route an explicitly composed melee ability through its inherited input method.
//! The sword override also reads raw fire, independently of its embedded input.
use super::*;

const ABILITY: u32 = 0xE610_E0D3;
const SWORD_ABILITY: u32 = 0x8080_41B1;
const ABILITY_BASE: u32 = 0x8080_3BB7;
const INPUT_METHOD: usize = 11;

pub(super) fn author(
    manager: &PackageManager,
    extension: &Extension,
    entity: &mut [u8],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<()> {
    let bindings = weapon_component_bindings(entity, ABILITY).map_err(invalid)?;
    let abilities = bindings
        .iter()
        .filter(|binding| extension.owners.contains(&binding.owner_tag))
        .collect::<Vec<_>>();
    let [ability] = abilities.as_slice() else {
        return Err(invalid(
            "Secondary input requires one composed melee ability",
        ));
    };
    if ability.concrete_class != SWORD_ABILITY {
        return Err(invalid("Secondary input has an unsupported melee ability"));
    }
    let maps = rows(entity, 0x58, 40, 0x8080_9C22)?;
    let map = *maps
        .get(ability.descriptor_index)
        .ok_or_else(|| invalid("Melee ability callback mapping is missing"))?;
    let template = TagHash(read_u32(entity, map + 32)?);
    let entry = manager
        .get_entry(template)
        .ok_or_else(|| invalid("Melee ability callback wrapper is missing"))?;
    let mut wrapper = read_tag(manager, template, "melee ability callback")?;
    if entry.reference != 0x8080_9C54
        || read_u64(&wrapper, 0)? != wrapper.len() as u64
        || read_u32(&wrapper, 8)? != 0x8080_3BEA
        || read_u32(&wrapper, 12)? != 0x8080_3C0D
    {
        return Err(invalid(
            "Melee ability callback has an unsupported interface",
        ));
    }
    let methods = rows(&wrapper, 0x10, 24, 0x8080_9C56)?;
    if methods.len() != 71 {
        return Err(invalid(
            "Melee ability callback has an unsupported method count",
        ));
    }
    let method = methods[INPUT_METHOD];
    if read_u32(&wrapper, method)? != SWORD_ABILITY
        || read_u32(&wrapper, method + 4)? != 9
        || wrapper[method + 8..method + 24]
            .iter()
            .any(|byte| *byte != 0)
        || read_u32(&wrapper, methods[10])? != ABILITY_BASE
        || read_u32(&wrapper, methods[10] + 4)? != 26
        || read_u32(&wrapper, methods[12])? != ABILITY_BASE
        || read_u32(&wrapper, methods[12] + 4)? != 28
    {
        return Err(invalid("Melee ability input method contract differs"));
    }
    // This inherited callback has the same ability/current/previous signature.
    // It consumes the ability command slot, without the sword's raw-fire path.
    wrapper[method..method + 4].copy_from_slice(&ABILITY_BASE.to_le_bytes());
    wrapper[method + 4..method + 8].copy_from_slice(&27u32.to_le_bytes());
    let assigned = allocator.assigned_tag(tags.len(), "Secondary input", "ability callback")?;
    tags.push(NewTagSpec {
        template_tag: template,
        payload: wrapper,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    entity[map + 32..map + 36].copy_from_slice(&assigned.0.to_le_bytes());
    Ok(())
}

fn rows(bytes: &[u8], descriptor: usize, stride: usize, class: u32) -> AuthoringResult<Vec<usize>> {
    let count = usize::try_from(read_u64(bytes, descriptor)?)
        .map_err(|_| invalid("Callback array count is too large"))?;
    let header = input::relative(bytes, descriptor + 8)?;
    let start = header
        .checked_add(16)
        .ok_or_else(|| invalid("Callback array header overflows"))?;
    let end = count
        .checked_mul(stride)
        .and_then(|length| start.checked_add(length))
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| invalid("Callback array exceeds its payload"))?;
    if count == 0
        || read_u64(bytes, header)? != count as u64
        || read_u32(bytes, header + 8)? != class
    {
        return Err(invalid("Callback array has an unsupported layout"));
    }
    Ok((start..end).step_by(stride).collect())
}
