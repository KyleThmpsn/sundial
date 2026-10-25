//! Health filters preserve symbolic categories and their compiled predicates.
use super::*;
use crate::d2_mot::native::effects::resources::selectors;

fn dictionary(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    resources: &selectors::Resources,
) -> Result<()> {
    ensure!(
        source.u32(from + 4)? == 1 && source.u64(from + 8)? == 0,
        "health category dictionary reference differs"
    );
    let native = resources
        .tags
        .get(&source.u32(from)?)
        .context("health category dictionary is unbound")?;
    put(&mut output.0, to, &native.to_le_bytes())?;
    put(&mut output.0, to + 4, &0u32.to_le_bytes())
}

pub(super) fn labels(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    resources: &selectors::Resources,
) -> Result<()> {
    for operation in 0..4 {
        let field = from + operation * 16;
        let rows = source.array(field, 32, Some(0x80809787))?;
        ensure!(
            rows.len() <= 448 && (!rows.is_empty() || source.u64(field + 8)? == 0),
            "health category list extent differs"
        );
        append_array(
            &mut output.0,
            to + operation * 16,
            0x808094B3,
            &vec![0; rows.len() * 24],
            24,
        )?;
        let targets = output.array(to + operation * 16, 24, Some(0x808094B3))?;
        for (&row, &target) in rows.iter().zip(&targets) {
            let name = source.u32(row)?;
            ensure!(
                source.u32(row + 4)? == 0,
                "health category row flags differ"
            );
            let index = resources
                .names
                .iter()
                .position(|&candidate| candidate == name)
                .context("health category is absent from source dictionary")?;
            ensure!(
                resources.categories.get(index).is_some_and(Option::is_some),
                "health category {name:08X} has no native correspondence"
            );
            put(&mut output.0, target, &name.to_le_bytes())?;
            string(source, row + 8, output, target + 8)?;
            dictionary(source, row + 16, output, target + 16, resources)?;
        }
    }
    string(source, from + 64, output, to + 64)?;
    dictionary(source, from + 72, output, to + 72, resources)
}

pub(super) fn damage(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    resources: &selectors::Resources,
) -> Result<()> {
    for offset in [0, 16] {
        let rows = source.array(from + offset, 1, Some(0x8080655E))?;
        ensure!(
            rows.len() <= 32 && (!rows.is_empty() || source.u64(from + offset + 8)? == 0),
            "health damage-type list extent differs"
        );
        let values = rows
            .iter()
            .map(|&row| source.u8(row))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            values.iter().all(|&value| value <= 6),
            "health damage type requires translation"
        );
        append_array(&mut output.0, to + offset, 0x80806B02, &values, 1)?;
    }
    labels(source, from + 32, output, to + 32, resources)
}

pub(super) fn ability(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    resources: &selectors::Resources,
) -> Result<()> {
    ensure!(
        source.u32(from)? & !0xFF == 0 && source.u32(from + 4)? & !0xFF == 0,
        "health ability mask exceeds native classes"
    );
    ensure!(
        matches!(source.u64(from + 8)?, 0 | 1 | 255)
            && source.u8(from + 24)? <= 1
            && source.u8(from + 25)? <= 1
            && source.bytes::<6>(from + 26)? == [0; 6],
        "health ability predicate flags differ"
    );
    put(&mut output.0, to, &source.bytes::<32>(from)?)?;
    let gates = selectors::append_mask(source, from + 16, output, to + 16, resources)?;
    ensure!(
        gates.is_empty(),
        "health ability predicate has unbound categories"
    );
    Ok(())
}
