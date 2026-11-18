//! Preserve the active energy rate alongside the source property's script companion.
use super::*;

pub(super) fn lower(
    source: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    let body = source.pointer(row + 32)?;
    let source_pair = (
        source.u32(source.pointer(16)? - 4)?,
        source.u32(source.pointer(24)? - 4)?,
    );
    let native_pair = (
        native.u32(native.pointer(16)? - 4)?,
        native.u32(native.pointer(24)? - 4)?,
    );
    let movement =
        source_pair == (0x808027BE, 0x808029B5) && native_pair == (0x80804112, 0x80804113);
    ensure!(
        (movement
            || (source_pair == (0x808031E7, 0x80803210)
                && native_pair == (0x8080428A, 0x8080428B)))
            && source.u32(body - 4)? == 0x8080243C
            && source.u32(row + 8)? == 255
            && source.u32(row + 16)? == 255
            && source.u64(row + 24)? == 0,
        "Active energy rate requires an ungated corresponding ability property"
    );
    // Both complete typed callbacks forward the numeric recipient to their generic
    // handler, which replaces +14. Active-phase energy updates read that same rate.
    let mut words = [0; 8];
    words[0] = source.u32(body)?;
    let template_key = if movement {
        let candidates = native
            .array(native.pointer(24)? + 0x88, 56, Some(0x8080453B))?
            .into_iter()
            .filter(|&at| {
                native.pointer(at + 48).and_then(|m| native.u32(m + 4)).ok() == Some(0x8080452E)
            })
            .collect::<Vec<_>>();
        let [witness] = candidates.as_slice() else {
            anyhow::bail!("Movement active rate has no unique native rate witness");
        };
        native.u32(*witness + 16)?
    } else {
        source.u32(row)?
    };
    let value = sundial::package_authoring::ability_bank::FixedModifier {
        template_key,
        class: 0x8080452E,
        words: sundial::package_authoring::ability_bank::widened(words),
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
            words,
            selection: value.selection,
        },
        None,
    )])
}
