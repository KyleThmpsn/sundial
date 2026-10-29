//! Preserve the active energy rate alongside the source property's script companion.
use super::*;

pub(super) fn lower(
    source: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let body = source.pointer(row + 32)?;
    ensure!(
        source.u32(source.pointer(16)? - 4)? == 0x808031E7
            && source.u32(source.pointer(24)? - 4)? == 0x80803210
            && source.u32(body - 4)? == 0x8080243C
            && source.u32(row + 8)? == 255
            && source.u32(row + 16)? == 255
            && source.u64(row + 24)? == 0,
        "Active energy rate requires an ungated Stormtrance property"
    );
    // Both complete typed callbacks forward the numeric recipient to their generic
    // handler, which replaces +14. Active-phase energy updates read that same rate.
    let mut words = [0; 8];
    words[0] = source.u32(body)?;
    let value = sundial::package_authoring::ability_bank::FixedModifier {
        template_key: source.u32(row)?,
        class: 0x8080452E,
        words,
        selection: Some(sundial::package_authoring::ability_bank::FixedSelection {
            name: source.u32(row + 4)?,
            priority: 255,
            required_key: 0,
        }),
    };
    value.check_bank(&native.0).map_err(anyhow::Error::msg)?;
    Ok(vec![(
        AbilityRowModifier::Fixed {
            template_key: value.template_key,
            class: value.class,
            words: value.words,
            selection: value.selection,
        },
        None,
    )])
}
