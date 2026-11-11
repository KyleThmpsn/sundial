//! A whole-number count a grenade adds to, kept natively as a script parameter.
//!
//! Source `80802662` holds one Int32 operand. The bank runtime `808029ED` adds it to its integer
//! chain state without clamping (method two, `010AE920`). The native bank keeps that count as a
//! script parameter, which an ungated native row under the same key selects alone with reset
//! zero. The private row adds the operand to that parameter, as the source adds it to its state.
use super::*;
use sundial::package_authoring::ability_bank;

pub(super) fn lower(
    data: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let at = data.pointer(row + 32)?;
    ensure!(
        data.u32(data.pointer(16)? - 4)? == 0x808029ED
            && data.u32(data.pointer(24)? - 4)? == 0x80802979,
        "Source integer ability property belongs to another bank runtime"
    );
    let operand = i32::from_le_bytes(data.bytes::<4>(at)?);
    ensure!(
        (1..=16).contains(&operand),
        "Source integer ability property operand {operand} is outside 1 to 16"
    );
    let key = data.u32(row)?;
    let rows = ability_bank::property_rows(&native.0).map_err(anyhow::Error::msg)?;
    let modifiers = ability_bank::row_modifiers(&native.0).map_err(anyhow::Error::msg)?;
    let witnesses = rows
        .iter()
        .zip(&modifiers)
        .filter(|(r, (_, gated))| {
            r.key == key && r.modifier_class == ability_bank::SCRIPT_DEFINITION_CLASS && !gated
        })
        .map(|(r, _)| r)
        .collect::<Vec<_>>();
    let [witness] = witnesses.as_slice() else {
        anyhow::bail!("Source integer ability property has no unique native script witness");
    };
    let [parameter] = witness.parameters.as_slice() else {
        anyhow::bail!("Native integer witness selects several script parameters");
    };
    ensure!(
        parameter.reset == 0.0,
        "Native integer witness parameter does not reset to zero"
    );
    Ok(vec![(
        AbilityRowModifier::Parameter {
            name: parameter.name,
            reset: parameter.reset.to_bits(),
            value: f32::from(u8::try_from(operand)?).to_bits(),
            add: true,
            gate: None,
        },
        None,
    )])
}
