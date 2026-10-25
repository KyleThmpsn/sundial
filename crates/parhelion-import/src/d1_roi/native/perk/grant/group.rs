//! Super-specific numeric groups retain their original child set and flattened values.
use super::*;
use sundial::package_authoring::ability_bank::{GroupEntry, GroupModifier};

pub(super) fn lower(
    source: &Source,
    data: &Payload,
    row: usize,
    native: &Payload,
) -> Result<Vec<(AbilityRowModifier, Option<usize>)>> {
    ensure!(
        data.u32(row + 16)? == 255,
        "Grouped source property mode differs"
    );
    let m = data.pointer(row + 32)?;
    let children = data.array(m, 8, Some(0x8080299D))?;
    let inputs = data.array(data.pointer(24)? + 0xC0, 56, Some(0x808023B9))?;
    let first = usize::try_from(data.u32(m + 16)?)?;
    let count = children.len();
    ensure!(
        (1..=8).contains(&count)
            && data.u32(m + 20)? as usize == count
            && first
                .checked_add(count)
                .is_some_and(|end| end <= inputs.len()),
        "Source grouped numeric input range differs"
    );
    let mut entries = [GroupEntry::default(); 8];
    for (index, child) in children.into_iter().enumerate() {
        let at = data.pointer(child)?;
        let selector = data.u16(at)?;
        let change = data.f32(at + 4)?;
        ensure!(
            data.u16(at + 2)? == 0 && change.is_finite(),
            "Source grouped numeric flags"
        );
        let marker = at.checked_sub(4).context("Source group child marker")?;
        let (class, input, value) = match (data.u32(marker)?, selector) {
            (0x80802862, 0..=2) => (0x80804483, selector, change),
            (0x808022D6, 3) => (0x80804484, selector, change),
            // Source damage is a positive signed change to the multiplier. The
            // matching native super declaration stores the final damage factor.
            (0x80802C2D, 0x120) if change > 0.0 => (0x80804481, 32, 1.0 + change),
            _ => anyhow::bail!("Source grouped numeric selector has no native witness"),
        };
        scalar::validate_input(
            source,
            data,
            inputs[first + index],
            change,
            data.u8(at + 1)?,
        )?;
        entries[index] = GroupEntry {
            class,
            input,
            value: value.to_bits(),
        };
    }
    let body = GroupModifier {
        template_key: data.u32(row)?,
        count: count as u8,
        entries,
    };
    body.check_bank(&native.0).map_err(anyhow::Error::msg)?;
    Ok(vec![(
        AbilityRowModifier::Group {
            template_key: body.template_key,
            count: body.count,
            entries,
        },
        None,
    )])
}
