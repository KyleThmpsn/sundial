//! The Rise of Iron stat translator behind a source weapon's firing pattern, translated onto the
//! base's native table by [`crate::tiger::weapon::timing`].
use super::*;
use crate::tiger::weapon::timing::{RISE_OF_IRON, Translator, translate};

pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    export: &Value,
    donor: u32,
) -> Result<Value> {
    let (_, patterns) = source.table(0x8080339A)?;
    let index = usize::try_from(
        export["runtime"]["pattern_index"]
            .as_u64()
            .context("Source pattern index")?,
    )?;
    let pattern = *patterns
        .array(8, 32, Some(0x808033BC))?
        .get(index)
        .context("Source firing pattern")?;
    let key = patterns.u32(pattern + 8)?;
    let root = u32::from_str_radix(
        export["runtime"]["runtime_entity"]
            .as_str()
            .context("Source firing runtime")?,
        16,
    )?;
    let entity = source.read(root)?;
    let mut candidates = Vec::new();
    for r in entity.array(32, 12, Some(0x80800715))? {
        let tag = entity.u32(r)?;
        let p = source.read(tag)?;
        let d = p.pointer(24)?;
        if p.array(d + 0x60, 48, Some(0x80802945))
            .is_ok_and(|rows| !rows.is_empty())
        {
            candidates.push((tag, p));
        }
    }
    ensure!(
        candidates.len() == 1,
        "Source firing translator is missing or ambiguous"
    );
    let (owner, payload) = candidates.pop().context("Source firing translator")?;
    let stats = export["stats"]["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let hash = u32::try_from(row["hash"].as_u64()?).ok()?;
            Some((hash, row["value"].as_f64()?))
        })
        .collect::<Vec<_>>();
    let translator = Translator {
        owner,
        payload,
        key,
        dialect: &RISE_OF_IRON,
    };
    translate(&translator, native, donor, &stats)
}
