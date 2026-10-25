//! Native runtime allocation metadata for supported damage cores.
use super::super::allocation::Row;
use super::{PROGRAMS, Payload, put};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(super) fn emit(
    source: &Payload,
    inputs: &BTreeMap<usize, usize>,
    template: &Payload,
) -> Result<Payload> {
    ensure!(
        template.u64(0)? == template.0.len() as u64,
        "damage allocation size differs"
    );
    template.array(32, 40, Some(0x80808852))?;
    let d = source.pointer(24)?;
    let mut rows = Vec::new();
    for (offset, name) in PROGRAMS
        .into_iter()
        .zip([0x3F6B2003, 0x4949BEFD, 0xDD65C77C, 0xB9FAE951])
    {
        let count = *inputs
            .get(&(d + offset))
            .context("damage inline inputs missing")?;
        if count != 0 {
            rows.push(Row::named(
                name,
                u32::MAX,
                0,
                vec![Row::named(0x1D7C9056, 0x80809788, count, Vec::new())],
            ));
        }
    }
    for (field, name, class, leaf) in [
        (0x158, 0xBCDA49B5, 0x80803774, 0x8C7EAD7C),
        (0x160, 0x1BDC6D6E, 0x80803772, 0x437BCAA9),
    ] {
        if source.u64(d + field)? == 0 {
            continue;
        }
        let wrapper = source.pointer(d + field)?;
        let count = *inputs
            .get(&(wrapper + 24))
            .context("damage wrapper expression missing")?;
        let children = if count != 0 {
            vec![Row::input(leaf, count)]
        } else {
            Vec::new()
        };
        rows.push(Row::named(name, class, 1, children));
    }
    let mut output = Payload(
        template
            .0
            .get(..48)
            .context("damage allocation header")?
            .to_vec(),
    );
    Row::write(&rows, &mut output, 32)?;
    let len = output.0.len() as u64;
    put(&mut output.0, 0, &len.to_le_bytes())?;
    Ok(output)
}
