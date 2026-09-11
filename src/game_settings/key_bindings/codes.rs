//! Native key indices and modifier flags from Sunrise's account input table.
use super::{BindingModifier, NAMED_INPUTS, binding_modifier, modified_input, trim_input_name};

// The first 116 canonical names are in native input order. The following entries
// are the unbound sentinel and named-format aliases, not additional native keys.
pub(super) const INPUT_COUNT: usize = 116;

pub(super) fn input_name(code: u64) -> Option<String> {
    let code = u16::try_from(code).ok()?;
    let index = usize::from(code & 0xff);
    let name = *NAMED_INPUTS.get(index).filter(|_| index < INPUT_COUNT)?;
    let modifier = match code & 0xff00 {
        0 => return Some(name.to_owned()),
        0x100 => "alt",
        0x200 => "control",
        0x400 => "shift",
        _ => return None,
    };
    Some(format!("{modifier}+{name}"))
}

pub(super) fn input_code(input: &str) -> Option<u16> {
    let (modifier, key) = modified_input(input).map_or(
        (BindingModifier::None, trim_input_name(input)),
        |(modifier, key)| (binding_modifier(modifier), key),
    );
    let index = NAMED_INPUTS[..INPUT_COUNT]
        .iter()
        .position(|name| name.eq_ignore_ascii_case(key))?;
    let modifier = match modifier {
        BindingModifier::None => 0,
        BindingModifier::Alt => 0x100,
        BindingModifier::Control => 0x200,
        BindingModifier::Shift => 0x400,
    };
    Some(index as u16 | modifier)
}
